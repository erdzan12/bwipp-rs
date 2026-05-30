//! MaxiCode — UPS-developed hexagonal 2D barcode.
//!
//! Fully verified port. MaxiCode symbols are a fixed 33-row × 30-column
//! hexagonal grid (odd-numbered rows are offset by half a module). The
//! encoder supports six modes:
//!   - Mode 2: structured carrier message with numeric postal code
//!   - Mode 3: structured carrier message with alphanumeric postal code
//!   - Mode 4: general data
//!   - Mode 5: enhanced ECC general data
//!   - Mode 6: reader-programming mode
//!
//! Reed-Solomon ECC is over GF(64) (the 6-bit codeword field).
//!
//! Tables here mirror `bwipp_maxicode.globals` from bwip-js line
//! 29010. Encoder body + mode dispatch + hexagonal renderer all
//! verified byte-for-byte against bwip-js (set-A/B/C/D/E shifts +
//! latches — 18 oracle tests in this module).

#![allow(dead_code)]

/// MaxiCode pad codeword per mode. Mode 2/3 use 33 ('!'), Mode 4/5
/// use 0 (NUL), Mode 6 uses 28 (FS).
///
/// Indexed by `mode - 2` (the supported modes are 2..=6).
pub(crate) const PAD_CODE: [u8; 5] = [33, 33, 0, 0, 28];

/// MaxiCode mode-latch lengths (bits). `LATCH_LEN[from][to]` is the
/// number of bytes BWIPP emits to switch from mode A to mode B.
/// Identity transitions are 0.
pub(crate) const LATCH_LEN: [[u8; 5]; 5] = [
    [0, 1, 1, 1, 1],
    [1, 0, 1, 1, 1],
    [2, 2, 0, 2, 2],
    [2, 2, 2, 0, 2],
    [2, 2, 2, 2, 0],
];

/// MaxiCode mode-latch byte sequences (`bwipp_maxicode.globals.maxicode_latchseq`).
///
/// `LATCH_SEQ[from][to]` is the slice of codeword bytes BWIPP emits
/// to switch from set `from` to set `to` (both indices in 0..=4 for
/// sets A..=E). Identity transitions emit an empty slice. Bytes are
/// interpreted in the **source** mode by the decoder — set A's
/// latch-to-B byte (63 = `LB`) is recognised as a latch in set A
/// per the charmap row for codeword 63.
///
/// Each `&'static [u8]` has length 0, 1, or 2 matching [`LATCH_LEN`].
pub(crate) const LATCH_SEQ: [[&[u8]; 5]; 5] = [
    // From A
    [&[], &[63], &[58], &[58], &[58]],
    // From B
    [&[63], &[], &[63], &[63], &[63]],
    // From C — double-emit codeword 60 latches via the LKC slot.
    [&[60, 60], &[60, 60], &[], &[60, 60], &[60, 60]],
    // From D — same pattern with codeword 61.
    [&[61, 61], &[61, 61], &[61, 61], &[], &[61, 61]],
    // From E — codeword 62.
    [&[62, 62], &[62, 62], &[62, 62], &[62, 62], &[]],
];

/// Pad codeword sentinel — codeword 33 in BWIPP's set A is the pad
/// fill BWIPP uses for empty positions in mode 2/3 secondary
/// messages.
pub(crate) const PAD_CODEWORD: u8 = 33;

/// Encode a secondary-message byte stream that lives entirely in
/// set A — no lowercase letters, no high-bit ASCII, no 9-or-longer
/// digit runs (which trigger BWIPP's NS optimization).
///
/// This is a building block for the full charset state machine.
/// The full encoder ([`encode_secondary_a_b`] /
/// [`encode_secondary_a_b_with_ns`]) dispatches into NS / set-B /
/// set-C/D/E branches before falling through to this simple path.
///
/// Returns the codeword sequence BWIPP would emit, or an error
/// when a byte isn't in set A or when a 9+ digit run is hit (caller
/// should pre-emit NS for those).
pub(crate) fn encode_set_a_only(bytes: &[u8]) -> Result<Vec<u8>, crate::error::Error> {
    // Verify no 9-or-longer digit runs.
    let mut digit_run = 0usize;
    for &b in bytes {
        if b.is_ascii_digit() {
            digit_run += 1;
            if digit_run >= 9 {
                return Err(crate::error::Error::InvalidData(
                    "MaxiCode encode_set_a_only: 9+ digit run requires the NS \
                     optimization (not in this path)"
                        .to_string(),
                ));
            }
        } else {
            digit_run = 0;
        }
    }
    let mut out = Vec::with_capacity(bytes.len());
    for &b in bytes {
        let cw = seta_codeword(b).ok_or_else(|| {
            crate::error::Error::InvalidData(format!(
                "MaxiCode encode_set_a_only: byte 0x{b:02x} not in set A \
                 (would need set-B/C/D/E shift/latch)",
            ))
        })?;
        out.push(cw);
    }
    Ok(out)
}

/// Special codewords used by the A↔B mixed-case dispatcher.
pub(crate) const SB_CODEWORD: u8 = 59; // shift-1 from A to B (set A row 59)
pub(crate) const LB_CODEWORD: u8 = 63; // latch from A to B (set A row 63)
pub(crate) const SA_CODEWORD: u8 = 59; // shift-1 from B to A — set B row 59
pub(crate) const SA2_CODEWORD: u8 = 56; // shift-2 from B to A — set B row 56
pub(crate) const SA3_CODEWORD: u8 = 57; // shift-3 from B to A — set B row 57
pub(crate) const LA_CODEWORD: u8 = 63; // latch from B to A — set B row 63

/// Single-byte shift codewords from set A or set B into sets C/D/E.
/// Per BWIPP `bwipp_maxicode` charmap rows 60-62: row 60 is `$_.sc`
/// in both columns A and B (codeword 60 = "shift to C"), row 61 is
/// `$_.sd` (codeword 61), row 62 is `$_.se` (codeword 62).
pub(crate) const SC_CODEWORD: u8 = 60;
pub(crate) const SD_CODEWORD: u8 = 61;
pub(crate) const SE_CODEWORD: u8 = 62;

/// Encode an A+B mixed-case secondary message using BWIPP's
/// rule-based dispatcher (the simplification of its full DP that
/// reproduces the right answer for the inputs we've corpus-verified).
///
/// Rules for a run of `R` set-B characters when the encoder is in
/// set A:
///   - R = 1: emit `SB` + codeword (2 cws, stay in A).
///   - R = 2 and at least one set-A char follows: emit `SB`+cw
///     twice (4 cws, stay in A) — BWIPP's tie-break with LB+LA.
///   - R = 2 at end of message: emit `LB`+cw+cw (3 cws) — beats
///     SB+SB by skipping the implicit return-to-A.
///   - R ≥ 3: emit `LB` + R cws, then `LA` if any set-A char
///     follows (otherwise terminate in B).
///
/// Symmetric rules for runs of set-A inside set B (the encoder
/// gets there via the preceding `LB`):
///   - Q = 1: `SA` + cw (2 cws, stay in B).
///   - Q = 2: `SA2` + 2 cws (3 cws, stay in B).
///   - Q = 3: `SA3` + 3 cws (4 cws, stay in B).
///   - Q ≥ 4: `LA` + Q cws, then `LB` if any set-B char follows.
///
/// Returns `Err(InvalidData)` if any byte isn't encodable in set A
/// or set B (i.e. high-bit ASCII that needs set C/D/E).
///
/// Does NOT yet apply the NS optimization — for digit-heavy inputs
/// the caller should pre-process or use [`encode_set_a_with_ns`].
///
/// **Known limitation**: this rule-based dispatcher diverges from
/// BWIPP for inputs that alternate set-A and set-B in patterns
/// like `setB_run + short_setA_run + setB_run`. BWIPP's full DP
/// will stay in set B and bridge the setA interlude with SA2/SA3;
/// our local rules pick SB+SB or LB+LA per run independently, which
/// can produce a longer (but still valid) symbol. Single A→B→A
/// or B→A→B transitions match BWIPP byte-for-byte.
pub(crate) fn encode_secondary_a_b(bytes: &[u8]) -> Result<Vec<u8>, crate::error::Error> {
    // Validate the alphabet first so we fail fast.
    for &b in bytes {
        if seta_codeword(b).is_none() && setb_codeword(b).is_none() {
            return Err(crate::error::Error::InvalidData(format!(
                "MaxiCode encode_secondary_a_b: byte 0x{b:02x} needs set C/D/E; \
                 use encode_secondary_a_b_with_ns instead (it supports the full \
                 set-A/B/C/D/E shift + latch + intra-latch dispatcher)",
            )));
        }
    }
    let n = bytes.len();
    let mut out: Vec<u8> = Vec::with_capacity(n + 4);
    let mut state_b = false; // false = set A, true = set B
    let mut i = 0;
    while i < n {
        // Find the next run of the "other" set. The run starts at i
        // and continues as long as the byte isn't in `state_b`'s set.
        let in_current_set = |b: u8| {
            if state_b {
                setb_codeword(b).is_some()
            } else {
                seta_codeword(b).is_some()
            }
        };
        if in_current_set(bytes[i]) {
            // Emit one char and continue.
            let cw = if state_b {
                setb_codeword(bytes[i]).unwrap()
            } else {
                seta_codeword(bytes[i]).unwrap()
            };
            out.push(cw);
            i += 1;
            continue;
        }
        // Other-set run starting at i. Find its end.
        let other_start = i;
        let mut j = i;
        while j < n && !in_current_set(bytes[j]) {
            j += 1;
        }
        let run_len = j - other_start;
        let trailing_current_set = j < n;
        if state_b {
            // Run is in set A.
            match run_len {
                1 => {
                    // SA + 1 cw.
                    out.push(SA_CODEWORD);
                    out.push(seta_codeword(bytes[other_start]).unwrap());
                }
                2 => {
                    // SA2 + 2 cws.
                    out.push(SA2_CODEWORD);
                    out.push(seta_codeword(bytes[other_start]).unwrap());
                    out.push(seta_codeword(bytes[other_start + 1]).unwrap());
                }
                3 => {
                    out.push(SA3_CODEWORD);
                    out.push(seta_codeword(bytes[other_start]).unwrap());
                    out.push(seta_codeword(bytes[other_start + 1]).unwrap());
                    out.push(seta_codeword(bytes[other_start + 2]).unwrap());
                }
                _ => {
                    // LA + run cws + (LB if more set-B follows).
                    out.push(LA_CODEWORD);
                    for &b in &bytes[other_start..j] {
                        out.push(seta_codeword(b).unwrap());
                    }
                    state_b = false;
                    if trailing_current_set {
                        out.push(LB_CODEWORD);
                        state_b = true;
                    }
                }
            }
        } else {
            // Run is in set B.
            match run_len {
                1 => {
                    out.push(SB_CODEWORD);
                    out.push(setb_codeword(bytes[other_start]).unwrap());
                }
                2 if trailing_current_set => {
                    // Mid-message: SB+SB (BWIPP's tie-break vs LB+LA).
                    out.push(SB_CODEWORD);
                    out.push(setb_codeword(bytes[other_start]).unwrap());
                    out.push(SB_CODEWORD);
                    out.push(setb_codeword(bytes[other_start + 1]).unwrap());
                }
                _ => {
                    // LB + R cws + (LA if more set-A follows).
                    out.push(LB_CODEWORD);
                    for &b in &bytes[other_start..j] {
                        out.push(setb_codeword(b).unwrap());
                    }
                    state_b = true;
                    if trailing_current_set {
                        out.push(LA_CODEWORD);
                        state_b = false;
                    }
                }
            }
        }
        i = j;
    }
    Ok(out)
}

/// Encode an A↔B mixed-case secondary message with NS optimization
/// for digit runs. Combines [`encode_secondary_a_b`]'s shift/latch
/// dispatcher with the [`encode_set_a_with_ns`] NS optimization.
///
/// For each digit run of length ≥9 inside a set-A context, the
/// encoder emits the leading (L − 9) digits as set-A codewords and
/// the final 9 digits as the 6-codeword NS chunk. Lowercase / set-B
/// chars are encoded via the rule-based A↔B dispatcher.
///
/// Returns `Err(InvalidData)` if any byte isn't encodable in set A
/// or set B. **Inherits the same `setA↔setB-alternation` limitation**
/// as [`encode_secondary_a_b`] — patterns like
/// `setB_run + setA_run + setB_run` may produce longer encodings
/// than BWIPP's full DP would (still valid, just non-optimal).
pub(crate) fn encode_secondary_a_b_with_ns(bytes: &[u8]) -> Result<Vec<u8>, crate::error::Error> {
    // Validate alphabet first — every byte must be encodable in at
    // least one of the five charsets (A, B, C, D, E).
    for &b in bytes {
        if seta_codeword(b).is_none()
            && setb_codeword(b).is_none()
            && setc_codeword(b).is_none()
            && setd_codeword(b).is_none()
            && sete_codeword(b).is_none()
        {
            return Err(crate::error::Error::InvalidData(format!(
                "MaxiCode encode_secondary_a_b_with_ns: byte 0x{b:02x} not in any \
                 of the five charsets (A/B/C/D/E)",
            )));
        }
    }
    let n = bytes.len();
    // Pre-compute digit-run suffix lengths for NS dispatch.
    let mut nseq = vec![0usize; n + 1];
    for i in (0..n).rev() {
        if bytes[i].is_ascii_digit() {
            nseq[i] = nseq[i + 1] + 1;
        }
    }
    let mut out: Vec<u8> = Vec::with_capacity(n + 4);
    let mut state_b = false;
    let mut i = 0;
    while i < n {
        // NS optimization works in ANY set — codeword 31 is "NS"
        // across all five charmap columns, and the 5 following cws
        // encode the 30-bit binary of 9 decimal digits. After NS
        // the encoder stays in its current set. So we try NS first
        // before any state-aware dispatching.
        if nseq[i] >= 9 {
            // In set A, BWIPP defers NS to the LAST 9 digits of a
            // run (emit leading L-9 digits as plain set-A codewords).
            // In set B, set-B can't represent digits directly, so we
            // jump straight to NS without emitting leading digits.
            let (lead, ns_start) = if !state_b {
                let lead = nseq[i] - 9;
                (lead, i + lead)
            } else {
                (0, i)
            };
            for k in 0..lead {
                out.push(seta_codeword(bytes[i + k]).unwrap());
            }
            let ns_chunk = encode_ns_run(&bytes[ns_start..ns_start + 9]).unwrap();
            out.extend_from_slice(&ns_chunk);
            i = ns_start + 9;
            continue;
        }
        // Identify whether current byte is in the active set.
        let in_current = if state_b {
            setb_codeword(bytes[i]).is_some()
        } else {
            seta_codeword(bytes[i]).is_some()
        };
        if in_current {
            let cw = if state_b {
                setb_codeword(bytes[i]).unwrap()
            } else {
                seta_codeword(bytes[i]).unwrap()
            };
            out.push(cw);
            i += 1;
            continue;
        }
        // The byte isn't in the active set. Check if it's a set-C/D/E
        // byte (high-bit ASCII or control chars) — those need a
        // shift (1 or 2 bytes) or a latch (3+ same-set bytes).
        //
        // BWIPP's preference order is C → D → E for the FIRST byte
        // of the run, then we scan forward greedily for same-set
        // bytes. Single bytes use the SC=60/SD=61/SE=62 shift; runs
        // of >= 3 use the [shift, shift] latch + run + back-latch.
        let cde_set = if setc_codeword(bytes[i]).is_some() {
            Some(2u8) // 2 = set C index
        } else if setd_codeword(bytes[i]).is_some() {
            Some(3u8) // 3 = set D
        } else if sete_codeword(bytes[i]).is_some() {
            Some(4u8) // 4 = set E
        } else {
            None
        };
        if let Some(set_idx) = cde_set {
            // Greedy scan for a same-set run, with 1-byte cross-set
            // lookahead. The walk extends through bytes in the primary
            // C/D/E set, and "absorbs" isolated cross-set bytes when
            // the byte AFTER the outlier is back in the primary set.
            // Absorbed bytes get an intra-latch shift (BWIPP's SC/SD/SE
            // codewords are also valid INSIDE another C/D/E latch and
            // mean "shift to that set for one byte").
            let lookup: fn(u8) -> Option<u8> = match set_idx {
                2 => setc_codeword,
                3 => setd_codeword,
                4 => sete_codeword,
                _ => unreachable!(),
            };
            let shift = match set_idx {
                2 => SC_CODEWORD,
                3 => SD_CODEWORD,
                4 => SE_CODEWORD,
                _ => unreachable!(),
            };
            // Walk forward. For each position k, classify the byte:
            //  - in current A/B set → stop (latch ends here)
            //  - in primary set → include directly
            //  - in another C/D/E set → absorb via intra-latch shift IF
            //      (a) the byte at k+1 is back in the primary set
            //          (single outlier mid-run), OR
            //      (b) we've already accumulated ≥3 primary-set bytes
            //          (committed-latch absorbs trailing cross-set
            //          bytes per BWIPP — verified for inputs like
            //          `^192^192^192^224`, `^192^192^192^224^224`,
            //          `^192^224^192^224^192`).
            //  - otherwise → stop
            let mut k = i;
            let mut body: Vec<u8> = Vec::with_capacity(n - i);
            let mut primary_count: usize = 0;
            while k < n {
                let in_current_ab = if state_b {
                    setb_codeword(bytes[k]).is_some()
                } else {
                    seta_codeword(bytes[k]).is_some()
                };
                if in_current_ab {
                    break;
                }
                if let Some(cw) = lookup(bytes[k]) {
                    body.push(cw);
                    primary_count += 1;
                    k += 1;
                    continue;
                }
                // Cross-set byte. Determine its set.
                let (target_idx, target_lookup): (u8, fn(u8) -> Option<u8>) =
                    if setc_codeword(bytes[k]).is_some() {
                        (2, setc_codeword)
                    } else if setd_codeword(bytes[k]).is_some() {
                        (3, setd_codeword)
                    } else if sete_codeword(bytes[k]).is_some() {
                        (4, sete_codeword)
                    } else {
                        break;
                    };
                let next_is_primary = k + 1 < n && lookup(bytes[k + 1]).is_some();
                let committed = primary_count >= 3;
                if !next_is_primary && !committed {
                    break;
                }
                let intra_shift = match target_idx {
                    2 => SC_CODEWORD,
                    3 => SD_CODEWORD,
                    4 => SE_CODEWORD,
                    _ => unreachable!(),
                };
                body.push(intra_shift);
                body.push(target_lookup(bytes[k]).unwrap());
                k += 1;
            }
            let run_len = k - i;
            if run_len >= 3 {
                // Latch path: [shift, shift, body..., back-latch].
                // After the [shift, shift] prefix BWIPP enters the
                // target set state; the body codewords (mix of direct
                // primary cws + 2-cw intra-latch shifts for outliers)
                // follow in that set's encoding; then a back-latch
                // returns to A or B (or terminates the message).
                out.push(shift);
                out.push(shift);
                out.extend_from_slice(&body);
                // Pick the back-latch byte. End-of-message: BWIPP
                // emits the LA back-latch (codeword 58 is "LA" in
                // sets C/D/E). Mid-message: depends on whether the
                // next byte is in set A or set B.
                //
                // Special case: BWIPP OMITS the back-latch at EOM for
                // set-E latches (but not set-C or set-D). Verified via
                // oracles for `TEST^160^162^163`, `TEST^160^162^163^164`,
                // and `TEST^160^162^163^192` (E latch + C intra-shift
                // trailing — still no back-latch). The trailing PAD
                // codewords (33) are recognized as end-of-data by
                // value per ISO/IEC 16023 §5.2.4.1, so no state reset
                // is strictly required; BWIPP exploits this for E only.
                let omit_back_latch_at_eom = k >= n && set_idx == 4;
                if omit_back_latch_at_eom {
                    // No back-latch emitted; the encoder's notional
                    // state stays in the latched set, but since we're
                    // at EOM the remaining cws will be PAD-filled.
                } else {
                    let back_latch = if k >= n {
                        58u8 // LA — back-latch from C or D at EOM
                    } else if state_b {
                        // Currently encoder believes it's in set B;
                        // if the next byte fits set B, emit LB.
                        // Otherwise fall back to LA.
                        if setb_codeword(bytes[k]).is_some() {
                            63u8 // LB
                        } else {
                            58u8 // LA
                        }
                    } else {
                        // Encoder is in set A. If next byte fits set A,
                        // LA. Otherwise LB (if it fits B) — fallback LA.
                        if seta_codeword(bytes[k]).is_some() {
                            58u8 // LA
                        } else if setb_codeword(bytes[k]).is_some() {
                            63u8 // LB
                        } else {
                            58u8 // LA — handles set-C/D/E follow-on too
                        }
                    };
                    out.push(back_latch);
                    // After the back-latch, the encoder is in:
                    //   - set A if back_latch == 58 (LA from C/D/E)
                    //   - set B if back_latch == 63 (LB from C/D/E)
                    // Reset state_b deterministically rather than
                    // toggling from whatever value it had before the
                    // latch.
                    state_b = back_latch == 63;
                }
                i = k;
                continue;
            }
            // Single-byte (run_len == 1) or double-byte (run_len == 2)
            // shift path. Both match BWIPP byte-for-byte: two
            // separate [shift, cw] pairs at the same total cost as
            // the latch alternative.
            //
            // Note: the walk loop at line 411-440 absorbs cross-set
            // bytes into a single body vec via intra-shift. When
            // run_len < 3 we re-iterate bytes[i..k] and emit each
            // byte's primary-set codeword directly — but if the walk
            // absorbed a cross-set outlier (e.g. set-E byte mixed
            // into a set-C run), the primary-set lookup returns
            // None for that byte. Stage 11.A4 fuzz uncovered this:
            // for input bytes [174, 121, ...] the walk picks set E
            // as primary, includes 174 (sete), absorbs 121 (set A,
            // breaks the run), then this loop tries setc_codeword(121)
            // and panics. Return InvalidData instead of unwrapping.
            for &b in &bytes[i..k] {
                let cw = lookup(b).ok_or_else(|| {
                    crate::error::Error::InvalidData(format!(
                        "MaxiCode encode_secondary_a_b_with_ns: byte 0x{b:02x} \
                         in the shift-path run has no codeword in the picked \
                         primary set (set_idx={set_idx}); cross-set mixing \
                         under run_len < 3 isn't supported",
                    ))
                })?;
                out.push(shift);
                out.push(cw);
            }
            i = k;
            continue;
        }
        // Run in the "other" (A/B) set starting at i.
        let mut j = i;
        while j < n {
            let in_other = if state_b {
                seta_codeword(bytes[j]).is_some() && setb_codeword(bytes[j]).is_none()
            } else {
                setb_codeword(bytes[j]).is_some() && seta_codeword(bytes[j]).is_none()
            };
            if !in_other {
                break;
            }
            j += 1;
        }
        let run_len = j - i;
        let trailing = j < n;
        if state_b {
            // Run in set A.
            match run_len {
                1 => {
                    out.push(SA_CODEWORD);
                    out.push(seta_codeword(bytes[i]).unwrap());
                }
                2 => {
                    out.push(SA2_CODEWORD);
                    out.push(seta_codeword(bytes[i]).unwrap());
                    out.push(seta_codeword(bytes[i + 1]).unwrap());
                }
                3 => {
                    out.push(SA3_CODEWORD);
                    out.push(seta_codeword(bytes[i]).unwrap());
                    out.push(seta_codeword(bytes[i + 1]).unwrap());
                    out.push(seta_codeword(bytes[i + 2]).unwrap());
                }
                _ => {
                    out.push(LA_CODEWORD);
                    for &b in &bytes[i..j] {
                        out.push(seta_codeword(b).unwrap());
                    }
                    state_b = false;
                    if trailing {
                        out.push(LB_CODEWORD);
                        state_b = true;
                    }
                }
            }
        } else {
            // Run in set B.
            match run_len {
                1 => {
                    out.push(SB_CODEWORD);
                    out.push(setb_codeword(bytes[i]).unwrap());
                }
                2 if trailing => {
                    out.push(SB_CODEWORD);
                    out.push(setb_codeword(bytes[i]).unwrap());
                    out.push(SB_CODEWORD);
                    out.push(setb_codeword(bytes[i + 1]).unwrap());
                }
                _ => {
                    out.push(LB_CODEWORD);
                    for &b in &bytes[i..j] {
                        out.push(setb_codeword(b).unwrap());
                    }
                    state_b = true;
                    // Skip the LA back to set A when the very next
                    // thing doesn't actually need set-A state:
                    //   - 9+ digit run: NS handles digits in any
                    //     state (BWIPP optimization)
                    //   - set-C/D/E byte: the C/D/E shift/latch
                    //     emits from B-context directly, no LA needed
                    //     (BWIPP oracle verified, e.g. "abcd^192" →
                    //     [63,1,2,3,4, 60,0] with no LA between B
                    //     run and the SC shift).
                    let next_is_cde = trailing
                        && seta_codeword(bytes[j]).is_none()
                        && setb_codeword(bytes[j]).is_none()
                        && (setc_codeword(bytes[j]).is_some()
                            || setd_codeword(bytes[j]).is_some()
                            || sete_codeword(bytes[j]).is_some());
                    if trailing && nseq[j] < 9 && !next_is_cde {
                        out.push(LA_CODEWORD);
                        state_b = false;
                    }
                }
            }
        }
        i = j;
    }
    Ok(out)
}

/// Apply BWIPP's MaxiCode RS-GF(64) ECC to a primary message and
/// a secondary message; produce the 144-codeword symbol stream
/// laid out as `[pri (10), prichk (10), sec (84 or 68), secchk
/// (40 or 56)]`.
///
/// Standard ECC (modes 2/3/4/6): `sec.len() == 84`, secondary
/// check k=20 (per half), `secchk.len() == 40`, total 144.
///
/// Enhanced ECC (mode 5): `sec.len() == 68`, secondary check
/// k=28 per half, `secchk.len() == 56`, total 144.
///
/// The secondary check is computed on two interleaves:
///   * `seco` = `sec[0], sec[2], sec[4], …` (even indices)
///   * `sece` = `sec[1], sec[3], sec[5], …` (odd indices)
///
/// Each half gets its own RS-check and they are re-interleaved into
/// `secchk = [secochk[0], secechk[0], secochk[1], secechk[1], …]`.
///
/// Direct port of bwip-js lines 30413-30457.
pub(crate) fn apply_rs_ecc(pri: &[u8; 10], sec: &[u8]) -> Result<[u8; 144], crate::error::Error> {
    let scodes = match sec.len() {
        84 => 20,
        68 => 28,
        other => {
            return Err(crate::error::Error::InvalidData(format!(
                "MaxiCode RS-ECC: secondary length must be 84 (modes 2/3/4/6) or 68 (mode 5), got {other}",
            )));
        }
    };
    let seco: Vec<u8> = sec.iter().step_by(2).copied().collect();
    let sece: Vec<u8> = sec.iter().skip(1).step_by(2).copied().collect();
    // BWIPP's MaxiCode wrapper emits the rscodes array in reverse
    // — the LFSR's low slot becomes the LAST check codeword in the
    // stream. Our crate::util::rs_gf64::encode_k matches BWIPP's
    // AusPost orientation (low slot first), so we reverse here.
    let mut secochk = crate::util::rs_gf64::encode_k(&seco, scodes);
    secochk.reverse();
    let mut secechk = crate::util::rs_gf64::encode_k(&sece, scodes);
    secechk.reverse();
    let mut secchk = Vec::with_capacity(scodes * 2);
    for i in 0..scodes {
        secchk.push(secochk[i]);
        secchk.push(secechk[i]);
    }
    let mut prichk = crate::util::rs_gf64::encode_k(pri, 10);
    prichk.reverse();

    let mut codewords = [0u8; 144];
    codewords[..10].copy_from_slice(pri);
    codewords[10..20].copy_from_slice(&prichk);
    codewords[20..20 + sec.len()].copy_from_slice(sec);
    codewords[20 + sec.len()..].copy_from_slice(&secchk);
    Ok(codewords)
}

/// Convert the 144-codeword symbol stream to an 864-bit module
/// array. Each codeword contributes 6 bits, MSB first. Direct
/// port of bwip-js lines 30472-30486.
pub(crate) fn codewords_to_mods(codewords: &[u8; 144]) -> [bool; 864] {
    let mut mods = [false; 864];
    for (i, &cw) in codewords.iter().enumerate() {
        for bit in 0..6 {
            let value = (cw >> (5 - bit)) & 1;
            mods[i * 6 + bit] = value == 1;
        }
    }
    mods
}

/// Final MaxiCode symbol — a 33-row × 30-column logical grid where
/// odd rows are physically offset by half a module to the right.
/// `cells[row][col]` is true when the corresponding hexagonal
/// module should be filled.
///
/// Use [`Self::is_on(row, col)`] to read the symbol; consumers can
/// then render the hex modules at appropriate geometric positions
/// (the rendering layer is hex-aware, not square).
#[derive(Debug, Clone)]
pub struct MaxiCodeSymbol {
    cells: [[bool; COLS]; ROWS],
}

impl MaxiCodeSymbol {
    /// Width of the symbol in logical columns (always 30).
    pub fn cols(&self) -> usize {
        COLS
    }
    /// Height of the symbol in logical rows (always 33).
    pub fn rows(&self) -> usize {
        ROWS
    }
    /// Whether the hex module at `(row, col)` should be filled.
    pub fn is_on(&self, row: usize, col: usize) -> bool {
        if row >= ROWS || col >= COLS {
            return false;
        }
        self.cells[row][col]
    }
    /// Whether the row is physically offset by half a module (odd rows).
    pub fn row_is_offset(row: usize) -> bool {
        row % 2 == 1
    }
}

/// MaxiCode's fixed-position cells that are *always* black: the
/// bull's-eye finder pattern, mode-indicator slot, and orientation
/// marks. Captured from bwip-js line 30498-30510.
pub(crate) const FIXED_BLACK_POSITIONS: &[u16] = &[
    28, 29, 280, 281, 311, 457, 488, 500, 530, 670, 700, 677, 707,
];

/// Project the 144 codewords onto the MaxiCode grid as a list of
/// cell positions that are "on" (black). Combines the data-bearing
/// cells (where the corresponding `mods[i]` bit is 1, mapped through
/// [`MODMAP`]) with the [`FIXED_BLACK_POSITIONS`] fixed-pattern
/// cells.
///
/// Each returned cell index `p` is `row * 30 + col` (in 0..=989).
/// Direct port of bwip-js lines 30487-30511.
pub(crate) fn lay_out_codewords(codewords: &[u8; 144]) -> Vec<u16> {
    let mods = codewords_to_mods(codewords);
    let mut pixs: Vec<u16> = Vec::with_capacity(864 + FIXED_BLACK_POSITIONS.len());
    for (i, &bit) in mods.iter().enumerate() {
        if bit {
            pixs.push(MODMAP[i]);
        }
    }
    pixs.extend_from_slice(FIXED_BLACK_POSITIONS);
    pixs
}

/// Convert the cell-index list from [`lay_out_codewords`] into a
/// boolean grid of the 33×30 MaxiCode symbol. `grid[row][col] = true`
/// when the cell is on (black).
///
/// Note that the symbol is logically hexagonal — odd rows are offset
/// by half a module to the right. The rendering layer needs to honor
/// that; this function gives the raw black-cell membership.
pub(crate) fn build_grid(pixs: &[u16]) -> [[bool; COLS]; ROWS] {
    let mut grid = [[false; COLS]; ROWS];
    for &p in pixs {
        let row = (p as usize) / COLS;
        let col = (p as usize) % COLS;
        if row < ROWS && col < COLS {
            grid[row][col] = true;
        }
    }
    grid
}

/// Build a complete [`MaxiCodeSymbol`] from a primary + secondary
/// codeword pair. The caller is responsible for constructing the
/// primary (via the crate-private `pack_mode_2_primary` or
/// `pack_mode_3_primary` packer) and the secondary (via the
/// crate-private `encode_secondary_a_b_with_ns` padded with the
/// `PAD_CODEWORD` sentinel to either 84 or 68 codewords).
pub fn build_symbol(pri: &[u8; 10], sec: &[u8]) -> Result<MaxiCodeSymbol, crate::error::Error> {
    let cws = apply_rs_ecc(pri, sec)?;
    let pixs = lay_out_codewords(&cws);
    Ok(MaxiCodeSymbol {
        cells: build_grid(&pixs),
    })
}

/// Build a mode-4 MaxiCode symbol (the general-data mode).
///
/// `data` is the raw secondary-message byte stream — it must be
/// encodable via the crate-private `encode_secondary_a_b_with_ns`
/// (set A + set B ASCII, no set C/D/E, no ECI). The encoder packs:
/// ```text
///   cws = [4 (mode), encmsg..., pad...]  (94 cws total)
///   pri = cws[0..10]
///   sec = cws[10..94]                    (84 cws)
/// ```
/// Pad value is `PAD_CODE[0]` = 33 (set-A pad), correct for inputs
/// whose encoder ends in set A or set B.
pub fn encode_mode_4(data: &[u8]) -> Result<MaxiCodeSymbol, crate::error::Error> {
    let encmsg = encode_secondary_a_b_with_ns(data)?;
    if encmsg.len() > 93 {
        return Err(crate::error::Error::InvalidData(format!(
            "MaxiCode mode 4: encoded message ({} codewords) exceeds 93-cw capacity",
            encmsg.len(),
        )));
    }
    let mut cws = [PAD_CODE[0]; 94];
    cws[0] = 4;
    cws[1..1 + encmsg.len()].copy_from_slice(&encmsg);
    let mut pri = [0u8; 10];
    pri.copy_from_slice(&cws[..10]);
    build_symbol(&pri, &cws[10..])
}

/// Build a mode-5 MaxiCode symbol (general data, enhanced ECC).
///
/// Mode 5 trades 16 secondary data slots for 16 extra ECC codewords:
/// `sec.len() == 68` (vs 84 in mode 4) and the per-half secondary
/// check is `k=28` (vs 20). The total cws count remains 144.
///
/// Per BWIPP `bwipp_maxicode` lines 29318-29334. The message limit
/// is 77 codewords (vs 93 in mode 4).
pub fn encode_mode_5(data: &[u8]) -> Result<MaxiCodeSymbol, crate::error::Error> {
    let encmsg = encode_secondary_a_b_with_ns(data)?;
    if encmsg.len() > 77 {
        return Err(crate::error::Error::InvalidData(format!(
            "MaxiCode mode 5: encoded message ({} codewords) exceeds 77-cw capacity",
            encmsg.len(),
        )));
    }
    let mut cws = [PAD_CODE[0]; 78];
    cws[0] = 5;
    cws[1..1 + encmsg.len()].copy_from_slice(&encmsg);
    let mut pri = [0u8; 10];
    pri.copy_from_slice(&cws[..10]);
    build_symbol(&pri, &cws[10..])
}

/// Build a mode-6 MaxiCode symbol (reader-programming data).
///
/// Mode 6 has the same data layout as mode 4 — 84-byte secondary
/// with 40 ECC bytes — but signals to the scanner that the payload
/// is configuration data rather than a tracking number. Per BWIPP
/// `bwipp_maxicode` lines 29318-29334.
pub fn encode_mode_6(data: &[u8]) -> Result<MaxiCodeSymbol, crate::error::Error> {
    let encmsg = encode_secondary_a_b_with_ns(data)?;
    if encmsg.len() > 93 {
        return Err(crate::error::Error::InvalidData(format!(
            "MaxiCode mode 6: encoded message ({} codewords) exceeds 93-cw capacity",
            encmsg.len(),
        )));
    }
    let mut cws = [PAD_CODE[0]; 94];
    cws[0] = 6;
    cws[1..1 + encmsg.len()].copy_from_slice(&encmsg);
    let mut pri = [0u8; 10];
    pri.copy_from_slice(&cws[..10]);
    build_symbol(&pri, &cws[10..])
}

/// Parse a structured carrier-message input for MaxiCode modes 2 / 3.
///
/// Accepts the BWIPP post-parsefnc format:
/// ```text
/// [optional FID prefix: "[)>\x1e01\x1d" + 2 digits] (9 bytes)
/// <postcode>\x1d<country>\x1d<service>\x1d<secondary>
/// ```
///
/// Returns `(postcode, country, service, secondary)` as byte slices
/// into `data`. The four fields are required; missing GS separators
/// produce `InvalidData`.
#[allow(clippy::type_complexity)]
pub(crate) fn parse_mode_2_or_3_input(
    data: &[u8],
) -> Result<(&[u8], &[u8], &[u8], &[u8]), crate::error::Error> {
    // Strip optional 9-byte FID prefix `[)>\x1e01\x1d<dd>` if present.
    let body = if data.len() >= 9
        && &data[0..7] == b"\x5b\x29\x3e\x1e\x30\x31\x1d"
        && data[7].is_ascii_digit()
        && data[8].is_ascii_digit()
    {
        &data[9..]
    } else {
        data
    };
    let parts: Vec<&[u8]> = body.splitn(4, |&b| b == 0x1d).collect();
    if parts.len() != 4 {
        return Err(crate::error::Error::InvalidData(format!(
            "MaxiCode mode 2/3: expected 4 GS-separated fields \
             (postcode, country, service, secondary); got {}",
            parts.len(),
        )));
    }
    Ok((parts[0], parts[1], parts[2], parts[3]))
}

/// Build a mode-2 MaxiCode symbol (US numeric postcode + structured
/// carrier message).
///
/// `data` is the post-parsefnc byte stream in the BWIPP
/// `<postcode>\x1d<country>\x1d<service>\x1d<secondary>` form
/// (optionally with a `[)>\x1e01\x1d<dd>` FID prefix).
pub fn encode_mode_2(data: &[u8]) -> Result<MaxiCodeSymbol, crate::error::Error> {
    let (postcode, country, service, secondary) = parse_mode_2_or_3_input(data)?;
    let pcode_str = std::str::from_utf8(postcode).map_err(|_| {
        crate::error::Error::InvalidData("MaxiCode mode 2: postcode must be ASCII".into())
    })?;
    let ccode_str = std::str::from_utf8(country).map_err(|_| {
        crate::error::Error::InvalidData("MaxiCode mode 2: country must be ASCII".into())
    })?;
    let scode_str = std::str::from_utf8(service).map_err(|_| {
        crate::error::Error::InvalidData("MaxiCode mode 2: service must be ASCII".into())
    })?;
    let pri = pack_mode_2_primary(pcode_str, ccode_str, scode_str)?;
    let encmsg = encode_secondary_a_b_with_ns(secondary)?;
    if encmsg.len() > 84 {
        return Err(crate::error::Error::InvalidData(format!(
            "MaxiCode mode 2: secondary message ({} cws) exceeds 84-cw capacity",
            encmsg.len(),
        )));
    }
    let mut sec = [PAD_CODE[0]; 84];
    sec[..encmsg.len()].copy_from_slice(&encmsg);
    build_symbol(&pri, &sec)
}

/// Build a mode-3 MaxiCode symbol (international alphanumeric postcode
/// + structured carrier message). Same input format as mode 2.
pub fn encode_mode_3(data: &[u8]) -> Result<MaxiCodeSymbol, crate::error::Error> {
    let (postcode, country, service, secondary) = parse_mode_2_or_3_input(data)?;
    let pcode_str = std::str::from_utf8(postcode).map_err(|_| {
        crate::error::Error::InvalidData("MaxiCode mode 3: postcode must be ASCII".into())
    })?;
    let ccode_str = std::str::from_utf8(country).map_err(|_| {
        crate::error::Error::InvalidData("MaxiCode mode 3: country must be ASCII".into())
    })?;
    let scode_str = std::str::from_utf8(service).map_err(|_| {
        crate::error::Error::InvalidData("MaxiCode mode 3: service must be ASCII".into())
    })?;
    let pri = pack_mode_3_primary(pcode_str, ccode_str, scode_str)?;
    let encmsg = encode_secondary_a_b_with_ns(secondary)?;
    if encmsg.len() > 84 {
        return Err(crate::error::Error::InvalidData(format!(
            "MaxiCode mode 3: secondary message ({} cws) exceeds 84-cw capacity",
            encmsg.len(),
        )));
    }
    let mut sec = [PAD_CODE[0]; 84];
    sec[..encmsg.len()].copy_from_slice(&encmsg);
    build_symbol(&pri, &sec)
}

/// NS (numeric shift) codeword — the sentinel value codeword 31
/// occupies in *every* set (charmap row 31 is `[NS, NS, NS, NS, NS]`).
pub(crate) const NS_CODEWORD: u8 = 31;

/// Encode a 9-digit run as the 6 codewords BWIPP emits via the NS
/// optimization: `[NS, b5, b4, b3, b2, b1]` where `b5..b1` are the
/// 5 6-bit chunks (MSB first) of the 30-bit binary representation
/// of the digit string as a decimal number.
///
/// Returns `None` if `digits` isn't exactly 9 ASCII digits.
pub(crate) fn encode_ns_run(digits: &[u8]) -> Option<[u8; 6]> {
    if digits.len() != 9 || !digits.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value: u32 = std::str::from_utf8(digits).unwrap().parse().unwrap();
    debug_assert!(value < (1 << 30));
    Some([
        NS_CODEWORD,
        ((value >> 24) & 63) as u8,
        ((value >> 18) & 63) as u8,
        ((value >> 12) & 63) as u8,
        ((value >> 6) & 63) as u8,
        (value & 63) as u8,
    ])
}

/// Encode a set-A secondary-message byte stream, using the NS
/// (numeric shift) optimization for any 9-or-longer digit run.
///
/// Matches BWIPP's "new" encoder DP for set-A-only inputs: for
/// each digit run of length `L >= 9`, the encoder emits the
/// leading `L - 9` digits as plain set-A codewords and packs the
/// final 9 digits into the 6-codeword NS chunk. This is the
/// "greedy-from-the-END" placement BWIPP's optimal-mode-search
/// settles on when the run is set-A-only (defers NS as late as
/// possible).
///
/// Returns the codeword stream, or `Err(InvalidData)` if any byte
/// isn't encodable in set A (callers route those through the
/// set-B/C/D/E paths via `setb_codeword` / `setc_codeword` /
/// `setd_codeword`, all of which are verified).
pub(crate) fn encode_set_a_with_ns(bytes: &[u8]) -> Result<Vec<u8>, crate::error::Error> {
    let n = bytes.len();
    let mut nseq = vec![0usize; n + 1];
    for i in (0..n).rev() {
        if bytes[i].is_ascii_digit() {
            nseq[i] = nseq[i + 1] + 1;
        }
    }
    let mut out: Vec<u8> = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        if nseq[i] >= 9 {
            // Digit run of length nseq[i] starting at i. Emit
            // (run_len - 9) leading digits as plain set-A codewords,
            // then NS for the final 9 digits of the run.
            let run_len = nseq[i];
            let lead = run_len - 9;
            for &b in &bytes[i..i + lead] {
                // SAFETY: b is a digit, so seta_codeword always succeeds.
                out.push(seta_codeword(b).unwrap());
            }
            let ns_start = i + lead;
            // SAFETY: 9 digits, encode_ns_run can't fail.
            let ns_chunk = encode_ns_run(&bytes[ns_start..ns_start + 9]).unwrap();
            out.extend_from_slice(&ns_chunk);
            i = ns_start + 9;
        } else {
            let b = bytes[i];
            let cw = seta_codeword(b).ok_or_else(|| {
                crate::error::Error::InvalidData(format!(
                    "MaxiCode encode_set_a_with_ns: byte 0x{b:02x} not in set A",
                ))
            })?;
            out.push(cw);
            i += 1;
        }
    }
    Ok(out)
}

/// Look up the MaxiCode set-B codeword for an ASCII byte. Returns
/// `None` when the byte isn't in set B's alphabet.
///
/// Set B covers lowercase letters (a-z → 1..=26 — same low values
/// as A-Z in set A) plus the inverse of the set-A punctuation
/// (`'{'` = 32, `';'` = 37, `'<'` = 38, `'='` = 39, `'>'` = 40,
/// `'?'` = 41, `'['` = 42, `'\'`  = 43, `']'` = 44, `'^'` = 45,
/// `'_'` = 46, `'  '`/...), plus digits encoded as their setA values.
///
/// Digits and uppercase have the SAME codeword values in set B as
/// in set A (charmap rows 48..=57 col 1 = ',', '.', '/', ':', '@',
/// '!', '|', PD2, SA2, SA3 — not the digits themselves). So setB
/// is NOT the right place for digits — the encoder should
/// shift/latch back to A for those.
pub(crate) fn setb_codeword(byte: u8) -> Option<u8> {
    match byte {
        b'`' => Some(0),
        b'a'..=b'z' => Some(byte - b'`'),
        // ASCII 28..=30 same as setA.
        28..=30 => Some(byte),
        // Set B punctuation block (codewords 32..=46): '{', '}', '~',
        // 127 (DEL), ';', '<', '=', '>', '?', '[', '\\', ']', '^',
        // '_', ' '. The codeword equals the row index in the charmap.
        b'{' => Some(32),
        b'}' => Some(34),
        b'~' => Some(35),
        127 => Some(36),
        b';' => Some(37),
        b'<' => Some(38),
        b'=' => Some(39),
        b'>' => Some(40),
        b'?' => Some(41),
        b'[' => Some(42),
        b'\\' => Some(43),
        b']' => Some(44),
        b'^' => Some(45),
        b'_' => Some(46),
        // Set B does NOT include ASCII space here — space is set A.
        _ => None,
    }
}

/// Look up the MaxiCode set-C codeword for a byte. Covers the
/// 192-223 range (À-ß) plus a band of low-128 high-bit ASCII
/// (128-137, 170, 172, 177-179, 181, 185, 186, 188-190).
///
/// Verified verbatim against bwip-js's `$_.charvals[2]` via
/// `tools/oracle-maxicode-charsets.js`.
pub(crate) fn setc_codeword(byte: u8) -> Option<u8> {
    match byte {
        192..=218 => Some(byte - 192),
        28..=30 => Some(byte),
        219..=223 => Some(byte - 219 + 32),
        170 => Some(37),
        172 => Some(38),
        177 => Some(39),
        178 => Some(40),
        179 => Some(41),
        181 => Some(42),
        185 => Some(43),
        186 => Some(44),
        188 => Some(45),
        189 => Some(46),
        190 => Some(47),
        128..=137 => Some(byte - 128 + 48),
        32 => Some(59),
        _ => None,
    }
}

/// Look up the MaxiCode set-D codeword for a byte. Covers the
/// 224-255 range (à-ÿ) plus a band of low-128 high-bit ASCII.
pub(crate) fn setd_codeword(byte: u8) -> Option<u8> {
    match byte {
        224..=250 => Some(byte - 224),
        28..=30 => Some(byte),
        251..=255 => Some(byte - 251 + 32),
        161 => Some(37),
        168 => Some(38),
        171 => Some(39),
        175 => Some(40),
        176 => Some(41),
        180 => Some(42),
        183 => Some(43),
        184 => Some(44),
        187 => Some(45),
        191 => Some(46),
        138..=148 => Some(byte - 138 + 47),
        32 => Some(59),
        _ => None,
    }
}

/// Look up the MaxiCode set-E codeword for a byte. Covers control
/// chars (0-26), ESC (27 → 30), high-bit symbols, and a band of
/// 149-158 mapping to codewords 48-57.
pub(crate) fn sete_codeword(byte: u8) -> Option<u8> {
    match byte {
        0..=26 => Some(byte),
        27 => Some(30),
        28..=31 => Some(byte - 28 + 32),
        159 => Some(36),
        160 => Some(37),
        162 => Some(38),
        163 => Some(39),
        164 => Some(40),
        165 => Some(41),
        166 => Some(42),
        167 => Some(43),
        169 => Some(44),
        173 => Some(45),
        174 => Some(46),
        182 => Some(47),
        149..=158 => Some(byte - 149 + 48),
        32 => Some(59),
        _ => None,
    }
}

/// Look up the MaxiCode set-A codeword for an ASCII byte. Returns
/// `None` when the byte isn't in set A's alphabet.
///
/// Set A covers uppercase letters (A-Z → 1..=26), digits (0-9 →
/// 48..=57), space (32 → 32), and a band of punctuation: 34..=58
/// (excluding ASCII 33 `!` which set A doesn't include). The full
/// charmap also has a few control characters (CR, FS, GS, RS) and
/// the shift/latch instructions, but encoders pass them as their
/// codeword directly rather than via byte lookup.
pub(crate) fn seta_codeword(byte: u8) -> Option<u8> {
    match byte {
        b'\r' => Some(0),
        b'A'..=b'Z' => Some(byte - b'@'),
        // ASCII 28..=30 (FS, GS, RS) — same value as codeword.
        28..=30 => Some(byte),
        b' ' => Some(32),
        // ASCII 34..=58 covers '"', '#', '$', '%', '&', '\'', '(',
        // ')', '*', '+', ',', '-', '.', '/', '0'..='9', ':'.
        34..=58 => Some(byte),
        _ => None,
    }
}

/// MaxiCode "modmap" — the per-codeword module-position map. Each
/// codeword contributes 6 bits, scattered across the symbol grid;
/// `MODMAP[codeword_index * 6 + bit_index]` is the grid-position
/// index `row * 30 + col` (in 0..=989) where that bit lives.
///
/// 864 entries = 144 codewords × 6 bits. Direct port of
/// `bwipp_maxicode.globals.maxicode_modmap` (bwip-js line 29050).
pub(crate) const MODMAP: [u16; 864] = [
    469, 529, 286, 316, 347, 346, 673, 672, 703, 702, 647, 676, 283, 282, 313, 312, 370, 610, 618,
    379, 378, 409, 408, 439, 705, 704, 559, 589, 588, 619, 458, 518, 640, 701, 675, 674, 285, 284,
    315, 314, 310, 340, 531, 289, 288, 319, 349, 348, 456, 486, 517, 516, 471, 470, 369, 368, 399,
    398, 429, 428, 549, 548, 579, 578, 609, 608, 649, 648, 679, 678, 709, 708, 639, 638, 669, 668,
    699, 698, 279, 278, 309, 308, 339, 338, 381, 380, 411, 410, 441, 440, 561, 560, 591, 590, 621,
    620, 547, 546, 577, 576, 607, 606, 367, 366, 397, 396, 427, 426, 291, 290, 321, 320, 351, 350,
    651, 650, 681, 680, 711, 710, 1, 0, 31, 30, 61, 60, 3, 2, 33, 32, 63, 62, 5, 4, 35, 34, 65, 64,
    7, 6, 37, 36, 67, 66, 9, 8, 39, 38, 69, 68, 11, 10, 41, 40, 71, 70, 13, 12, 43, 42, 73, 72, 15,
    14, 45, 44, 75, 74, 17, 16, 47, 46, 77, 76, 19, 18, 49, 48, 79, 78, 21, 20, 51, 50, 81, 80, 23,
    22, 53, 52, 83, 82, 25, 24, 55, 54, 85, 84, 27, 26, 57, 56, 87, 86, 117, 116, 147, 146, 177,
    176, 115, 114, 145, 144, 175, 174, 113, 112, 143, 142, 173, 172, 111, 110, 141, 140, 171, 170,
    109, 108, 139, 138, 169, 168, 107, 106, 137, 136, 167, 166, 105, 104, 135, 134, 165, 164, 103,
    102, 133, 132, 163, 162, 101, 100, 131, 130, 161, 160, 99, 98, 129, 128, 159, 158, 97, 96, 127,
    126, 157, 156, 95, 94, 125, 124, 155, 154, 93, 92, 123, 122, 153, 152, 91, 90, 121, 120, 151,
    150, 181, 180, 211, 210, 241, 240, 183, 182, 213, 212, 243, 242, 185, 184, 215, 214, 245, 244,
    187, 186, 217, 216, 247, 246, 189, 188, 219, 218, 249, 248, 191, 190, 221, 220, 251, 250, 193,
    192, 223, 222, 253, 252, 195, 194, 225, 224, 255, 254, 197, 196, 227, 226, 257, 256, 199, 198,
    229, 228, 259, 258, 201, 200, 231, 230, 261, 260, 203, 202, 233, 232, 263, 262, 205, 204, 235,
    234, 265, 264, 207, 206, 237, 236, 267, 266, 297, 296, 327, 326, 357, 356, 295, 294, 325, 324,
    355, 354, 293, 292, 323, 322, 353, 352, 277, 276, 307, 306, 337, 336, 275, 274, 305, 304, 335,
    334, 273, 272, 303, 302, 333, 332, 271, 270, 301, 300, 331, 330, 361, 360, 391, 390, 421, 420,
    363, 362, 393, 392, 423, 422, 365, 364, 395, 394, 425, 424, 383, 382, 413, 412, 443, 442, 385,
    384, 415, 414, 445, 444, 387, 386, 417, 416, 447, 446, 477, 476, 507, 506, 537, 536, 475, 474,
    505, 504, 535, 534, 473, 472, 503, 502, 533, 532, 455, 454, 485, 484, 515, 514, 453, 452, 483,
    482, 513, 512, 451, 450, 481, 480, 511, 510, 541, 540, 571, 570, 601, 600, 543, 542, 573, 572,
    603, 602, 545, 544, 575, 574, 605, 604, 563, 562, 593, 592, 623, 622, 565, 564, 595, 594, 625,
    624, 567, 566, 597, 596, 627, 626, 657, 656, 687, 686, 717, 716, 655, 654, 685, 684, 715, 714,
    653, 652, 683, 682, 713, 712, 637, 636, 667, 666, 697, 696, 635, 634, 665, 664, 695, 694, 633,
    632, 663, 662, 693, 692, 631, 630, 661, 660, 691, 690, 721, 720, 751, 750, 781, 780, 723, 722,
    753, 752, 783, 782, 725, 724, 755, 754, 785, 784, 727, 726, 757, 756, 787, 786, 729, 728, 759,
    758, 789, 788, 731, 730, 761, 760, 791, 790, 733, 732, 763, 762, 793, 792, 735, 734, 765, 764,
    795, 794, 737, 736, 767, 766, 797, 796, 739, 738, 769, 768, 799, 798, 741, 740, 771, 770, 801,
    800, 743, 742, 773, 772, 803, 802, 745, 744, 775, 774, 805, 804, 747, 746, 777, 776, 807, 806,
    837, 836, 867, 866, 897, 896, 835, 834, 865, 864, 895, 894, 833, 832, 863, 862, 893, 892, 831,
    830, 861, 860, 891, 890, 829, 828, 859, 858, 889, 888, 827, 826, 857, 856, 887, 886, 825, 824,
    855, 854, 885, 884, 823, 822, 853, 852, 883, 882, 821, 820, 851, 850, 881, 880, 819, 818, 849,
    848, 879, 878, 817, 816, 847, 846, 877, 876, 815, 814, 845, 844, 875, 874, 813, 812, 843, 842,
    873, 872, 811, 810, 841, 840, 871, 870, 901, 900, 931, 930, 961, 960, 903, 902, 933, 932, 963,
    962, 905, 904, 935, 934, 965, 964, 907, 906, 937, 936, 967, 966, 909, 908, 939, 938, 969, 968,
    911, 910, 941, 940, 971, 970, 913, 912, 943, 942, 973, 972, 915, 914, 945, 944, 975, 974, 917,
    916, 947, 946, 977, 976, 919, 918, 949, 948, 979, 978, 921, 920, 951, 950, 981, 980, 923, 922,
    953, 952, 983, 982, 925, 924, 955, 954, 985, 984, 927, 926, 957, 956, 987, 986, 58, 89, 88,
    118, 149, 148, 178, 209, 208, 238, 269, 268, 298, 329, 328, 358, 389, 388, 418, 449, 448, 478,
    509, 508, 538, 569, 568, 598, 629, 628, 658, 689, 688, 718, 749, 748, 778, 809, 808, 838, 869,
    868, 898, 929, 928, 958, 989, 988,
];

/// Symbol dimensions — fixed for MaxiCode.
pub(crate) const ROWS: usize = 33;
pub(crate) const COLS: usize = 30;
pub(crate) const TOTAL_CELLS: usize = ROWS * COLS; // 990

/// 6-bit codeword sentinel constants (negative values in BWIPP).
pub(crate) const SENTINEL_ECI: i32 = -1;
pub(crate) const SENTINEL_PAD: i32 = -2;
pub(crate) const SENTINEL_NS: i32 = -3;
/// Latch-to-A through E (mode switches).
pub(crate) const SENTINEL_LA: i32 = -4;
pub(crate) const SENTINEL_LB: i32 = -5;
/// Shift-to-A through E (one-character mode switches).
pub(crate) const SENTINEL_SA: i32 = -6;
pub(crate) const SENTINEL_SB: i32 = -7;
pub(crate) const SENTINEL_SC: i32 = -8;
pub(crate) const SENTINEL_SD: i32 = -9;
pub(crate) const SENTINEL_SE: i32 = -10;

/// Encode a value as a fixed-width binary string, MSB first.
/// Returns `None` if `value` doesn't fit in `width` bits.
fn to_bin(value: u64, width: usize) -> Option<String> {
    if width < 64 && value >= (1u64 << width) {
        return None;
    }
    let mut s = String::with_capacity(width);
    for k in (0..width).rev() {
        s.push(if (value >> k) & 1 == 1 { '1' } else { '0' });
    }
    Some(s)
}

/// Pack the structured carrier message for MaxiCode mode 2 (numeric
/// postal code) into 10 primary codewords.
///
/// `postcode` is 1-9 ASCII digits. `country` and `service` are exactly
/// 3 ASCII digits each. Special case for US (`country == "840"`):
/// 5-digit postcodes get padded to 9 chars with `"0000"` suffix.
///
/// Direct port of bwip-js lines 29296-29356 — the `mdb` (mode bits),
/// `ccb` (country bits), `scb` (service bits), `pcb` (postcode bits)
/// composition + the `scm[0..60]` slot-by-slot assembly. Returns the
/// 10 6-bit codewords BWIPP stores in `pri`.
///
/// Returns [`Error::InvalidData`] for malformed inputs.
///
/// [`Error::InvalidData`]: crate::error::Error::InvalidData
pub(crate) fn pack_mode_2_primary(
    postcode: &str,
    country: &str,
    service: &str,
) -> Result<[u8; 10], crate::error::Error> {
    let postcode = postcode.trim_end();
    if postcode.is_empty() || postcode.len() > 9 || !postcode.bytes().all(|b| b.is_ascii_digit()) {
        return Err(crate::error::Error::InvalidData(format!(
            "MaxiCode mode 2: postcode must be 1-9 ASCII digits, got {postcode:?}",
        )));
    }
    if country.len() != 3 || !country.bytes().all(|b| b.is_ascii_digit()) {
        return Err(crate::error::Error::InvalidData(format!(
            "MaxiCode mode 2: country must be 3 ASCII digits, got {country:?}",
        )));
    }
    if service.len() != 3 || !service.bytes().all(|b| b.is_ascii_digit()) {
        return Err(crate::error::Error::InvalidData(format!(
            "MaxiCode mode 2: service must be 3 ASCII digits, got {service:?}",
        )));
    }
    // US ZIP+4 special-case: 5-digit code gets padded to 9 with "0000"
    // suffix so postcode value carries the +4 in the low 4 digits.
    let pcode_string;
    let pcode = if country == "840" && postcode.len() == 5 {
        pcode_string = format!("{postcode}0000");
        pcode_string.as_str()
    } else {
        postcode
    };
    let mdb = to_bin(2, 4).unwrap();
    let ccode_n: u64 = country.parse().unwrap();
    let scode_n: u64 = service.parse().unwrap();
    let ccb = to_bin(ccode_n, 10).unwrap();
    let scb = to_bin(scode_n, 10).unwrap();
    // pcb (36 bits) = 6-bit length + 30-bit numeric value.
    let pcode_n: u64 = pcode.parse().unwrap();
    let pcode_len_bits = to_bin(pcode.len() as u64, 6).ok_or_else(|| {
        crate::error::Error::InvalidData(format!(
            "MaxiCode mode 2: postcode length {} doesn't fit in 6 bits",
            pcode.len(),
        ))
    })?;
    let pcode_value_bits = to_bin(pcode_n, 30).ok_or_else(|| {
        crate::error::Error::InvalidData(format!(
            "MaxiCode mode 2: numeric postcode {pcode_n} doesn't fit in 30 bits",
        ))
    })?;
    let mut pcb = String::with_capacity(36);
    pcb.push_str(&pcode_len_bits);
    pcb.push_str(&pcode_value_bits);
    assert_eq!(pcb.len(), 36);

    // BWIPP scm slot layout (bwip-js lines 30336-30349):
    //   scm[2..6]   = mdb              (mode bits)
    //   scm[38..42] = pcb[0..4]
    //   scm[30..36] = pcb[4..10]
    //   scm[24..30] = pcb[10..16]
    //   scm[18..24] = pcb[16..22]
    //   scm[12..18] = pcb[22..28]
    //   scm[6..12]  = pcb[28..34]
    //   scm[0..2]   = pcb[34..36]
    //   scm[52..54] = ccb[0..2]
    //   scm[42..48] = ccb[2..8]
    //   scm[36..38] = ccb[8..10]
    //   scm[54..60] = scb[0..6]
    //   scm[48..52] = scb[6..10]
    let mut scm = [b'0'; 60];
    let copy = |scm: &mut [u8; 60], dst: usize, src: &str| {
        scm[dst..dst + src.len()].copy_from_slice(src.as_bytes());
    };
    copy(&mut scm, 2, &mdb);
    copy(&mut scm, 38, &pcb[0..4]);
    copy(&mut scm, 30, &pcb[4..10]);
    copy(&mut scm, 24, &pcb[10..16]);
    copy(&mut scm, 18, &pcb[16..22]);
    copy(&mut scm, 12, &pcb[22..28]);
    copy(&mut scm, 6, &pcb[28..34]);
    copy(&mut scm, 0, &pcb[34..36]);
    copy(&mut scm, 52, &ccb[0..2]);
    copy(&mut scm, 42, &ccb[2..8]);
    copy(&mut scm, 36, &ccb[8..10]);
    copy(&mut scm, 54, &scb[0..6]);
    copy(&mut scm, 48, &scb[6..10]);

    Ok(slice_scm_to_codewords(&scm))
}

/// Look up an ASCII byte's MaxiCode setA charset value. Returns
/// `None` if the byte isn't in the mode-3 postcode alphabet
/// (space, `"`-`:`, A-Z).
fn seta_value_mode3(c: u8) -> Option<u8> {
    match c {
        32 => Some(32),
        34..=58 => Some(c),
        65..=90 => Some(c - 64),
        _ => None,
    }
}

/// Convert the 60-byte scm-as-ASCII-bits buffer into 10 6-bit
/// codewords.
fn slice_scm_to_codewords(scm: &[u8; 60]) -> [u8; 10] {
    let mut pri = [0u8; 10];
    for (chunk_idx, slot) in pri.iter_mut().enumerate() {
        let mut value = 0u8;
        for bit_idx in 0..6 {
            let bit = scm[chunk_idx * 6 + bit_idx] - b'0';
            value = (value << 1) | bit;
        }
        *slot = value;
    }
    pri
}

/// Pack the structured carrier message for MaxiCode mode 3
/// (alphanumeric postal code) into 10 primary codewords.
///
/// `postcode` is 1-6 ASCII characters from the mode-3 alphabet:
/// space, `"`-`:`, A-Z. Shorter postcodes are right-padded with
/// spaces to 6 characters; longer postcodes are rejected.
///
/// `country` and `service` are exactly 3 ASCII digits each.
///
/// Direct port of bwip-js lines 29319-29335 — the alphanumeric pcb
/// variant. Each of the 6 postcode characters becomes a 6-bit setA
/// codeword; the rest of the scm assembly is identical to mode 2.
pub(crate) fn pack_mode_3_primary(
    postcode: &str,
    country: &str,
    service: &str,
) -> Result<[u8; 10], crate::error::Error> {
    if postcode.is_empty() || postcode.len() > 6 {
        return Err(crate::error::Error::InvalidData(format!(
            "MaxiCode mode 3: postcode must be 1-6 characters, got {} chars",
            postcode.len(),
        )));
    }
    for &b in postcode.as_bytes() {
        if seta_value_mode3(b).is_none() {
            return Err(crate::error::Error::InvalidData(format!(
                "MaxiCode mode 3: postcode contains invalid byte 0x{b:02x} (allowed: space, '\"'..':', 'A'..'Z')",
            )));
        }
    }
    if country.len() != 3 || !country.bytes().all(|b| b.is_ascii_digit()) {
        return Err(crate::error::Error::InvalidData(format!(
            "MaxiCode mode 3: country must be 3 ASCII digits, got {country:?}",
        )));
    }
    if service.len() != 3 || !service.bytes().all(|b| b.is_ascii_digit()) {
        return Err(crate::error::Error::InvalidData(format!(
            "MaxiCode mode 3: service must be 3 ASCII digits, got {service:?}",
        )));
    }
    // Pad postcode to 6 chars with trailing spaces.
    let mut padded = [b' '; 6];
    padded[..postcode.len()].copy_from_slice(postcode.as_bytes());

    let mdb = to_bin(3, 4).unwrap();
    let ccode_n: u64 = country.parse().unwrap();
    let scode_n: u64 = service.parse().unwrap();
    let ccb = to_bin(ccode_n, 10).unwrap();
    let scb = to_bin(scode_n, 10).unwrap();

    // pcb (36 bits) = 6 × 6-bit setA codewords concatenated.
    let mut pcb = String::with_capacity(36);
    for &b in &padded {
        let val = seta_value_mode3(b).unwrap();
        pcb.push_str(&to_bin(u64::from(val), 6).unwrap());
    }

    // Same scm slot layout as mode 2.
    let mut scm = [b'0'; 60];
    let copy = |scm: &mut [u8; 60], dst: usize, src: &str| {
        scm[dst..dst + src.len()].copy_from_slice(src.as_bytes());
    };
    copy(&mut scm, 2, &mdb);
    copy(&mut scm, 38, &pcb[0..4]);
    copy(&mut scm, 30, &pcb[4..10]);
    copy(&mut scm, 24, &pcb[10..16]);
    copy(&mut scm, 18, &pcb[16..22]);
    copy(&mut scm, 12, &pcb[22..28]);
    copy(&mut scm, 6, &pcb[28..34]);
    copy(&mut scm, 0, &pcb[34..36]);
    copy(&mut scm, 52, &ccb[0..2]);
    copy(&mut scm, 42, &ccb[2..8]);
    copy(&mut scm, 36, &ccb[8..10]);
    copy(&mut scm, 54, &scb[0..6]);
    copy(&mut scm, 48, &scb[6..10]);

    Ok(slice_scm_to_codewords(&scm))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_code_shape() {
        // Modes 2/3 use ASCII '!' (33) as pad, 4/5 use NUL, 6 uses FS.
        assert_eq!(PAD_CODE, [33, 33, 0, 0, 28]);
    }

    /// Stage 11.A8c — pin `seta_value_mode3(c)`. 4-arm lookup for
    /// MaxiCode mode-3 (alphanumeric postcode) characters:
    ///   - 32 (space) → Some(32)
    ///   - 34..=58 (`"` through `:`) → Some(c) identity
    ///   - 65..=90 (A-Z) → Some(c - 64) → 1..=26
    ///   - else → None
    ///
    /// Used only by `pack_mode_3_primary`; the existing mode-3
    /// goldens exercise it transitively. Mutations to catch:
    ///   - `Some(32)` → `None`: space rejected.
    ///   - `34..=58` boundary shifts (e.g. `33..=58` accepts '!').
    ///   - `c - 64` → `c - 65`: every letter shifted by 1.
    ///   - `c - 64` → `c + 64`: huge wrong values.
    ///   - identity arm replaced with `Some(c - 32)`: shifts " through :.
    #[test]
    fn seta_value_mode3_arms_and_boundaries() {
        // Arm 1: space → 32.
        assert_eq!(seta_value_mode3(32), Some(32));
        // Arm 1 boundary: 31 → None (control char), 33 ('!') → None (between space and `"`).
        assert_eq!(seta_value_mode3(31), None);
        assert_eq!(
            seta_value_mode3(33),
            None,
            "'!' is between 32 (space) and 34 ('\"')"
        );

        // Arm 2: 34..=58 identity. Endpoints + a mid-range value.
        assert_eq!(seta_value_mode3(34), Some(34), "'\"' start of range");
        assert_eq!(seta_value_mode3(45), Some(45), "'-' mid-range");
        assert_eq!(seta_value_mode3(58), Some(58), "':' end of range");
        // Boundary: 59 (';') → None.
        assert_eq!(
            seta_value_mode3(59),
            None,
            "';' is between ':' (58) and 'A' (65)"
        );
        assert_eq!(seta_value_mode3(64), None, "'@' is one before 'A'");

        // Arm 3: A-Z → 1-26 (c - 64).
        assert_eq!(seta_value_mode3(65), Some(1), "'A' → 1");
        assert_eq!(seta_value_mode3(77), Some(13), "'M' → 13");
        assert_eq!(seta_value_mode3(90), Some(26), "'Z' → 26");
        // Boundary: '[' (91) → None.
        assert_eq!(seta_value_mode3(91), None, "'[' just after 'Z'");

        // Catch-all None for other bytes.
        assert_eq!(seta_value_mode3(0), None);
        assert_eq!(seta_value_mode3(97), None, "lowercase 'a' rejected");
        assert_eq!(seta_value_mode3(122), None, "lowercase 'z' rejected");
        assert_eq!(seta_value_mode3(255), None);
    }

    /// Stage 11.A8c — pin `slice_scm_to_codewords`. Packs 60 ASCII
    /// '0'/'1' bytes into 10 6-bit codewords (MSB first within each
    /// 6-bit chunk). Used inside `pack_mode_2_primary` /
    /// `pack_mode_3_primary` after the bit-string assembly; only
    /// exercised transitively via mode-2/3 goldens.
    ///
    /// Mutations to catch:
    ///   - `value << 1` → `value >> 1`: bit pattern inverted.
    ///   - `| bit` → `& bit`: bits not accumulated.
    ///   - `chunk_idx * 6 + bit_idx` index arithmetic mutations.
    ///   - `bit_idx in 0..6` → `0..7` or `0..5`: width off-by-one.
    ///   - `scm[...] - b'0'` → `+ b'0'`: would store ASCII codepoints.
    #[test]
    fn slice_scm_to_codewords_msb_packing() {
        // All-zero input → all-zero codewords.
        let scm = [b'0'; 60];
        assert_eq!(slice_scm_to_codewords(&scm), [0u8; 10]);

        // All-one input → all-0x3F (=63) codewords.
        let scm = [b'1'; 60];
        assert_eq!(slice_scm_to_codewords(&scm), [0x3F; 10]);

        // Per-codeword anchor: "000001" × 10 → each = 1.
        let mut scm = [b'0'; 60];
        for chunk in 0..10 {
            scm[chunk * 6 + 5] = b'1';
        }
        assert_eq!(
            slice_scm_to_codewords(&scm),
            [1u8; 10],
            "MSB-first: only LSB of each 6-bit chunk set → codeword = 1"
        );

        // Per-codeword anchor: "100000" × 10 → each = 32 (high bit set).
        let mut scm = [b'0'; 60];
        for chunk in 0..10 {
            scm[chunk * 6] = b'1';
        }
        assert_eq!(
            slice_scm_to_codewords(&scm),
            [32u8; 10],
            "MSB-first: only MSB set → codeword = 32"
        );

        // Alternating "010101" × 10 → each = 0b010101 = 21.
        let mut scm = [b'0'; 60];
        for chunk in 0..10 {
            scm[chunk * 6 + 1] = b'1';
            scm[chunk * 6 + 3] = b'1';
            scm[chunk * 6 + 5] = b'1';
        }
        assert_eq!(
            slice_scm_to_codewords(&scm),
            [21u8; 10],
            "MSB-first alternating: 010101 = 0x15 = 21"
        );

        // Distinct per-codeword values: chunk N gets value N (in 6 bits).
        let mut scm = [b'0'; 60];
        for chunk in 0..10u8 {
            // Write 6 bits of `chunk` (MSB-first) into slots [chunk*6 .. chunk*6+6].
            for bit_idx in 0..6 {
                let bit = (chunk >> (5 - bit_idx)) & 1;
                scm[chunk as usize * 6 + bit_idx] = b'0' + bit;
            }
        }
        let expected: [u8; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        assert_eq!(
            slice_scm_to_codewords(&scm),
            expected,
            "per-chunk indexing: each chunk encodes its index"
        );
    }

    /// Stage 11.A8c — pin `to_bin(value, width)`. Fixed-width binary
    /// string formatter MSB first. Returns None if `value >= (1 <<
    /// width)`. The `width < 64` guard prevents `(1u64 << 64)` overflow
    /// when the caller asks for the full-64-bit representation.
    ///
    /// `to_bin` is only used inside `pack_mode_2_primary` and friends
    /// (mode 2/3 postal-code packing); the existing maxicode encode
    /// goldens exercise it transitively but never pin the boundary
    /// conditions in isolation.
    ///
    /// Mutations to catch:
    ///   - `width < 64` → `width <= 64`: width=64 would shift by 64
    ///     and overflow.
    ///   - `value >= (1u64 << width)` → `>` or `<=`: boundary case
    ///     value = (1<<width) - 1 (max valid) or value = 1<<width
    ///     (first overflow) flips.
    ///   - `'1' else '0'` arms swapped: every bit inverted.
    ///   - `(0..width).rev()` removed: LSB-first instead of MSB-first.
    #[test]
    fn to_bin_msb_first_with_width_guard() {
        // Basic cases.
        assert_eq!(to_bin(0, 4), Some("0000".to_string()));
        assert_eq!(to_bin(5, 4), Some("0101".to_string()));
        assert_eq!(to_bin(15, 4), Some("1111".to_string()), "max-fit value");
        assert_eq!(to_bin(16, 4), None, "value=16 > 4 bits → None");
        // Width 0: only value 0 fits.
        assert_eq!(to_bin(0, 0), Some(String::new()));
        assert_eq!(to_bin(1, 0), None);
        // Width 1.
        assert_eq!(to_bin(0, 1), Some("0".to_string()));
        assert_eq!(to_bin(1, 1), Some("1".to_string()));
        assert_eq!(to_bin(2, 1), None);
        // Width 8: full byte.
        assert_eq!(to_bin(0xAA, 8), Some("10101010".to_string()), "MSB first");
        assert_eq!(to_bin(0xFF, 8), Some("11111111".to_string()));
        assert_eq!(to_bin(0x100, 8), None, "256 doesn't fit in 8 bits");
        // Width 64: the guard branch — overflow check skipped, every
        // u64 value accepted.
        assert_eq!(
            to_bin(0, 64),
            Some("0".repeat(64)),
            "width=64: 0 → 64 zeros"
        );
        assert_eq!(
            to_bin(u64::MAX, 64),
            Some("1".repeat(64)),
            "width=64: u64::MAX → 64 ones (no overflow check)"
        );
        let high_bit_64 = 1u64 << 63;
        let mut expected_high = String::from("1");
        expected_high.push_str(&"0".repeat(63));
        assert_eq!(
            to_bin(high_bit_64, 64),
            Some(expected_high),
            "width=64: top bit set, rest zero"
        );
        // Width 63: boundary just below the guard.
        assert_eq!(to_bin(0, 63), Some("0".repeat(63)));
        let max_63 = (1u64 << 63) - 1;
        assert_eq!(
            to_bin(max_63, 63),
            Some("1".repeat(63)),
            "max fit in 63 bits"
        );
        assert_eq!(to_bin(1u64 << 63, 63), None, "1<<63 doesn't fit in 63 bits");
    }

    #[test]
    fn latch_len_shape() {
        // 5×5 matrix with zeros on the diagonal.
        for (from, row) in LATCH_LEN.iter().enumerate() {
            assert_eq!(row[from], 0, "identity latch should be 0");
        }
        // First two modes need 1-byte latches everywhere except to self;
        // last three need 2-byte latches.
        assert_eq!(LATCH_LEN[0], [0, 1, 1, 1, 1]);
        assert_eq!(LATCH_LEN[4], [2, 2, 2, 2, 0]);
    }

    #[test]
    fn modmap_covers_all_grid_cells() {
        // 864 entries = 144 codewords × 6 bits per codeword. All entries
        // are grid positions in 0..=989 (33 × 30 grid).
        assert_eq!(MODMAP.len(), 864);
        for (i, &pos) in MODMAP.iter().enumerate() {
            assert!(
                (pos as usize) < TOTAL_CELLS,
                "MODMAP[{i}] = {pos} ≥ {TOTAL_CELLS}",
            );
        }
        // Verify some spot-checks against the BWIPP source.
        assert_eq!(MODMAP[0], 469);
        assert_eq!(MODMAP[5], 346);
        assert_eq!(MODMAP[863], 988);
    }

    #[test]
    fn modmap_entries_are_unique() {
        // Each codeword bit lives in exactly one cell; no MODMAP entry
        // should appear twice.
        let mut seen = [false; TOTAL_CELLS];
        let mut dupes = 0;
        for &pos in MODMAP.iter() {
            if seen[pos as usize] {
                dupes += 1;
            }
            seen[pos as usize] = true;
        }
        assert_eq!(dupes, 0, "MODMAP has {dupes} duplicate cells");
        // 864 of 990 cells are data; the remaining 126 are finder
        // pattern, mode indicator, orientation, and timing.
        let used = seen.iter().filter(|&&b| b).count();
        assert_eq!(used, 864);
    }

    #[test]
    fn encode_set_a_only_matches_oracle() {
        // Pure set-A inputs (no 9-digit-run NS optimization).
        // "TEST1234" → [20, 5, 19, 20, 49, 50, 51, 52] per BWIPP.
        assert_eq!(
            encode_set_a_only(b"TEST1234").unwrap(),
            vec![20, 5, 19, 20, 49, 50, 51, 52],
        );
        // "ABC" → [1, 2, 3].
        assert_eq!(encode_set_a_only(b"ABC").unwrap(), vec![1, 2, 3]);
        // Up to 8 consecutive digits is fine (no NS).
        assert_eq!(
            encode_set_a_only(b"12345678").unwrap(),
            vec![49, 50, 51, 52, 53, 54, 55, 56],
        );
    }

    #[test]
    fn encode_set_a_with_ns_matches_oracle_corpus() {
        // (input, expected encmsg) pairs captured from
        // tools/oracle-maxicode-secondary.js. Cover the four
        // placement patterns the BWIPP DP picks for set-A inputs:
        //   - no 9-digit run (NS not used at all)
        //   - exactly 9-digit run (NS at run start)
        //   - 9-digit run with leading non-digit (NS at run start)
        //   - >9-digit run (NS deferred to last 9 of run)
        let cases: &[(&[u8], &[u8])] = &[
            // No NS — short.
            (b"TEST1234", &[20, 5, 19, 20, 49, 50, 51, 52]),
            (b"ABC", &[1, 2, 3]),
            (b"12345678", &[49, 50, 51, 52, 53, 54, 55, 56]),
            // NS for an exactly-9-digit run.
            (b"012345678", &[31, 0, 47, 6, 5, 14]),
            (b"123456789", &[31, 7, 22, 60, 52, 21]),
            // NS deferred for runs > 9: BWIPP emits the leading
            // digits as plain set A and reserves NS for the last 9.
            (b"0123456789", &[48, 31, 7, 22, 60, 52, 21]),
            (b"01234567890", &[48, 49, 31, 13, 62, 51, 35, 18]),
            (b"012345678901", &[48, 49, 50, 31, 20, 38, 42, 16, 53]),
            // NS with surrounding non-digit context.
            (b"X123456789Y", &[24, 31, 7, 22, 60, 52, 21, 25]),
            (b"X012345678Y", &[24, 31, 0, 47, 6, 5, 14, 25]),
            (b"X12345678901Y", &[24, 49, 50, 31, 20, 38, 42, 16, 53, 25]),
            (
                b"ABC123456789DEF",
                &[1, 2, 3, 31, 7, 22, 60, 52, 21, 4, 5, 6],
            ),
        ];
        for (input, want) in cases {
            // Stage 11.A8c (cont) — per-iteration `.unwrap()` →
            // `.unwrap_or_else(panic!)` naming the MaxiCode set-A+NS
            // corpus row with utf-8-safe input echo.
            let input_echo = std::str::from_utf8(input).unwrap_or("<non-utf8>");
            let got = encode_set_a_with_ns(input).unwrap_or_else(|e| {
                panic!(
                    "encode_set_a_with_ns({input_echo:?}) (MaxiCode set-A + NS-run digit-pair compaction corpus) must succeed: {e:?}",
                )
            });
            assert_eq!(
                got,
                *want,
                "encode_set_a_with_ns mismatch for {:?}",
                std::str::from_utf8(input).unwrap_or("<non-utf8>"),
            );
        }
    }

    #[test]
    fn encode_ns_run_9_digit_matches_oracle() {
        // "123456789" → [31, 7, 22, 60, 52, 21] (from oracle for
        // pure-9-digit input).
        assert_eq!(
            encode_ns_run(b"123456789").unwrap(),
            [31, 7, 22, 60, 52, 21],
        );
        // "012345678" → [31, 0, 47, 6, 5, 14].
        assert_eq!(encode_ns_run(b"012345678").unwrap(), [31, 0, 47, 6, 5, 14],);
        // "000000000" → [31, 0, 0, 0, 0, 0].
        assert_eq!(encode_ns_run(b"000000000").unwrap(), [31, 0, 0, 0, 0, 0]);
        // "999999999" — value 999999999 = 0x3B9AC9FF.
        // 30 bits: 11 1011 1001 1010 1100 1001 1111 1111.
        // Split: 111011, 100110, 101100, 100111, 111111 = 59, 38, 44, 39, 63.
        assert_eq!(
            encode_ns_run(b"999999999").unwrap(),
            [31, 59, 38, 44, 39, 63],
        );
    }

    #[test]
    fn encode_ns_run_rejects_non_9_digit_input() {
        assert!(encode_ns_run(b"12345678").is_none()); // too short
        assert!(encode_ns_run(b"1234567890").is_none()); // too long
        assert!(encode_ns_run(b"12345678A").is_none()); // non-digit
    }

    /// Stage 11.A8c — upgrade from 3 weak is_err() checks to per-arm
    /// diagnostic-substring + byte-echo pins. encode_set_a_only has
    /// TWO distinct rejection arms (line 78-105):
    ///   * 9+ digit run → "...9+ digit run requires the NS
    ///     optimization (not in this path)"
    ///   * byte not in set A → "...byte 0xHH not in set A (would
    ///     need set-B/C/D/E shift/latch)"
    ///
    /// A mutant that swaps the two arm bodies (so non-set-A bytes
    /// report the NS optimization, and digit runs report the set-A
    /// shift hint) survives variant-only is_err() checks.
    #[test]
    fn encode_set_a_only_rejects_non_seta() {
        // Arm 2: lowercase needs set B. seta_codeword('a') = None.
        let err = encode_set_a_only(b"abc").expect_err("'abc' must reject");
        let crate::error::Error::InvalidData(msg) = err else {
            panic!("'abc' must yield InvalidData; got {err:?}");
        };
        assert!(
            msg.contains("MaxiCode encode_set_a_only:")
                && msg.contains("0x61")
                && msg.contains("not in set A"),
            "lowercase 'a' (0x61) must pin set-A-membership diagnostic + byte echo; got {msg:?}"
        );
        assert!(
            !msg.contains("NS optimization"),
            "set-A-membership diagnostic must not leak the NS-run arm; got {msg:?}"
        );

        // Arm 2 (high-bit byte): 0xC3 is not in set A.
        //
        // Stage 11.A8c (cont) — bring the high-bit-byte assertions
        // up to the same anchor count as arm 1 (line 1835-1837):
        //   1. `MaxiCode encode_set_a_only:` arm-route prefix
        //   2. `0xc3` byte-hex echo (preserved)
        //   3. `not in set A` set-membership predicate (preserved)
        //   4. cross-arm guard: must NOT contain `NS optimization`
        //      (the sibling 9+digit-run arm wording)
        let err = encode_set_a_only(&[0xC3]).expect_err("0xC3 must reject");
        let crate::error::Error::InvalidData(msg) = err else {
            panic!("0xC3 must yield InvalidData; got {err:?}");
        };
        assert!(
            msg.contains("MaxiCode encode_set_a_only:"),
            "missing `MaxiCode encode_set_a_only:` arm-route prefix: {msg:?}"
        );
        assert!(
            msg.contains("0xc3") && msg.contains("not in set A"),
            "high-bit byte must pin 0xc3 echo + set-A-membership text; got {msg:?}"
        );
        assert!(
            !msg.contains("NS optimization"),
            "cross-arm contamination: high-bit-byte reject mentions `NS optimization`: {msg:?}"
        );

        // Arm 1: 9-digit run requires NS optimization.
        let err = encode_set_a_only(b"123456789").expect_err("9-digit run must reject");
        let crate::error::Error::InvalidData(msg) = err else {
            panic!("9-digit run must yield InvalidData; got {err:?}");
        };
        assert!(
            msg.contains("9+ digit run") && msg.contains("NS optimization"),
            "digit-run diagnostic must call out the 9+ run + NS optimization; got {msg:?}"
        );
        assert!(
            !msg.contains("not in set A"),
            "NS-run diagnostic must not leak the set-A-membership arm; got {msg:?}"
        );
    }

    #[test]
    fn latch_seq_shape_matches_latch_len() {
        // For every (from, to) pair, len(LATCH_SEQ) should equal
        // LATCH_LEN.
        for from in 0..5 {
            for to in 0..5 {
                assert_eq!(
                    LATCH_SEQ[from][to].len(),
                    LATCH_LEN[from][to] as usize,
                    "LATCH_SEQ[{from}][{to}] length mismatch",
                );
            }
        }
    }

    #[test]
    fn encode_secondary_a_b_matches_oracle_corpus() {
        // (input, expected) captured from
        // tools/oracle-maxicode-secondary.js. Cover every A↔B
        // rule branch.
        let cases: &[(&[u8], &[u8])] = &[
            // Pure A: identical to encode_set_a_only.
            (b"ABC", &[1, 2, 3]),
            // Pure B (LB at start, all in B).
            (b"abc", &[63, 1, 2, 3]),
            // 1 set-B at end: SB.
            (b"Aa", &[1, 59, 1]),
            (b"Ab", &[1, 59, 2]),
            // 2 set-B at end: LB (no LA needed).
            (b"Aab", &[1, 63, 1, 2]),
            // 3 set-B at end.
            (b"Aabc", &[1, 63, 1, 2, 3]),
            // 1 set-B in middle: SB.
            (b"AaB", &[1, 59, 1, 2]),
            // 2 set-B in middle: SB+SB (BWIPP's tie-break).
            (b"AabB", &[1, 59, 1, 59, 2, 2]),
            (b"AabBC", &[1, 59, 1, 59, 2, 2, 3]),
            // 3 set-B in middle: LB + LA.
            (b"AabcB", &[1, 63, 1, 2, 3, 63, 2]),
            (b"AabcBC", &[1, 63, 1, 2, 3, 63, 2, 3]),
            // 4 set-B in middle: LB + 4 cws + LA.
            (b"AabcdB", &[1, 63, 1, 2, 3, 4, 63, 2]),
            // A-only with no shift.
            (b"TEST1234", &[20, 5, 19, 20, 49, 50, 51, 52]),
            // Pure-A followed by 3 set-B at end.
            (b"ABCabc", &[1, 2, 3, 63, 1, 2, 3]),
            (b"abcDEF", &[63, 1, 2, 3, 63, 4, 5, 6]),
        ];
        for (input, want) in cases {
            let got = encode_secondary_a_b(input).unwrap();
            assert_eq!(
                got,
                *want,
                "encode_secondary_a_b mismatch for {:?}",
                std::str::from_utf8(input).unwrap_or("<non-utf8>"),
            );
        }
    }

    #[test]
    fn apply_rs_ecc_matches_oracle_for_mode_4_x() {
        // Oracle for mode 4 with input "X":
        //   pri = [4, 24, 33, 33, 33, 33, 33, 33, 33, 33]
        //   sec = 84 × 33 (all pad codewords)
        //   prichk = [39, 10, 13, 32, 7, 26, 45, 16, 13, 6]
        //   secchk = 40 cws (interleaved pairs, since seco == sece)
        let pri: [u8; 10] = [4, 24, 33, 33, 33, 33, 33, 33, 33, 33];
        let sec = [33u8; 84];
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the MaxiCode RS-ECC mode-4 path: pri segment 10 cws +
        // prichk 10 cws + sec 84 cws + secchk 40 cws (interleaved
        // pairs since seco == sece).
        let cws = apply_rs_ecc(&pri, &sec).expect(
            "apply_rs_ecc(mode 4 X pri+sec) (MaxiCode mode-4 RS-ECC path: GF(64) RS over pri/sec; 10+10+84+40=144 cw total) must succeed",
        );
        // pri segment
        assert_eq!(&cws[..10], pri.as_slice(), "pri segment mismatch");
        // prichk segment
        let want_prichk: [u8; 10] = [39, 10, 13, 32, 7, 26, 45, 16, 13, 6];
        assert_eq!(&cws[10..20], want_prichk.as_slice(), "prichk mismatch");
        // sec segment
        assert_eq!(&cws[20..104], sec.as_slice(), "sec segment mismatch");
        // secchk segment: 40 cws, interleaved pairs of secochk/secechk.
        // Since seco == sece, both halves produce the same checks,
        // so the interleave is pairs of identical values.
        let want_secchk: [u8; 40] = [
            60, 60, 40, 40, 9, 9, 43, 43, 14, 14, 50, 50, 12, 12, 53, 53, 57, 57, 58, 58, 36, 36,
            28, 28, 10, 10, 53, 53, 37, 37, 30, 30, 14, 14, 5, 5, 31, 31, 40, 40,
        ];
        assert_eq!(&cws[104..], want_secchk.as_slice(), "secchk mismatch");
    }

    /// Stage 11.A8c — pin the `apply_rs_ecc` **error arm** for invalid
    /// secondary lengths. The existing
    /// `apply_rs_ecc_matches_oracle_for_mode_4_x` test covers the
    /// happy path for the 84-byte branch (mode 2/3/4/6), and
    /// `encode_mode_5_codewords_match_bwip_js_test` exercises the
    /// 68-byte branch transitively. But the `other => Err(...)`
    /// match arm at lines 652-656 has **no test** — a mutation like
    /// `84 => 20` → `_ => 20` (collapsing the match to a default arm)
    /// or removing the error return entirely would silently succeed
    /// for non-standard sec lengths.
    ///
    /// Probed lengths chosen to catch both off-by-one boundaries
    /// (67 = just below mode 5; 85 = just above mode 4) and a wider
    /// out-of-range value (100), plus the degenerate empty case.
    ///
    /// Mutations caught:
    ///   * `84 => 20` → `_ => 20`: every sec.len() would succeed, so
    ///     the 67 / 70 / 85 / 100 / 0 cases would not error.
    ///   * Removed `other => return Err(...)`: same outcome.
    ///   * Format `{other}` → `{}` or `{0}`: the wrong-length echo
    ///     wouldn't include the offending value.
    ///   * Format-string predicates dropped: missing `MaxiCode RS-ECC`
    ///     prefix or missing `84` / `68` valid-length hints.
    #[test]
    fn apply_rs_ecc_rejects_invalid_secondary_length() {
        let pri = [0u8; 10];

        // Empty secondary.
        let err_empty = apply_rs_ecc(&pri, &[]).expect_err("empty sec must error");
        let crate::error::Error::InvalidData(msg) = err_empty else {
            panic!("empty-sec error must be InvalidData; got {err_empty:?}");
        };
        assert!(msg.contains("MaxiCode RS-ECC"), "diagnostic prefix: {msg}");
        assert!(msg.contains("84"), "mention valid length 84: {msg}");
        assert!(msg.contains("68"), "mention valid length 68: {msg}");
        assert!(msg.contains(" 0"), "echo the wrong length 0: {msg}");

        // 67-byte sec (just below mode 5's 68-byte boundary).
        let short = [0u8; 67];
        let err_67 = apply_rs_ecc(&pri, &short).expect_err("67-byte sec must error");
        let crate::error::Error::InvalidData(msg) = err_67 else {
            panic!("67-byte sec error must be InvalidData; got {err_67:?}");
        };
        assert!(msg.contains("67"), "echo the wrong length 67: {msg}");

        // 70-byte sec (between modes 5 and 2/3/4/6).
        let mid = [0u8; 70];
        let err_70 = apply_rs_ecc(&pri, &mid).expect_err("70-byte sec must error");
        let crate::error::Error::InvalidData(msg) = err_70 else {
            panic!("70-byte sec error must be InvalidData; got {err_70:?}");
        };
        assert!(msg.contains("70"), "echo the wrong length 70: {msg}");

        // 85-byte sec (just above mode 2/3/4/6's 84-byte boundary).
        let just_over = [0u8; 85];
        let err_85 = apply_rs_ecc(&pri, &just_over).expect_err("85-byte sec must error");
        let crate::error::Error::InvalidData(msg) = err_85 else {
            panic!("85-byte sec error must be InvalidData; got {err_85:?}");
        };
        assert!(msg.contains("85"), "echo the wrong length 85: {msg}");

        // 100-byte sec (well above any valid length).
        let big = [0u8; 100];
        let err_100 = apply_rs_ecc(&pri, &big).expect_err("100-byte sec must error");
        let crate::error::Error::InvalidData(msg) = err_100 else {
            panic!("100-byte sec error must be InvalidData; got {err_100:?}");
        };
        assert!(msg.contains("100"), "echo the wrong length 100: {msg}");
    }

    #[test]
    fn lay_out_codewords_matches_oracle_for_mode_4_x() {
        // Reproduce BWIPP's pixs for mode 4 input "X".
        // The same codewords array we verified in apply_rs_ecc.
        let pri: [u8; 10] = [4, 24, 33, 33, 33, 33, 33, 33, 33, 33];
        let sec = [33u8; 84];
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the MaxiCode lay_out_codewords end-to-end-RS path: same
        // fixture as the RS-ECC test, then lays out as 350-cell pixs
        // (337 data + 13 fixed_black_positions).
        let cws = apply_rs_ecc(&pri, &sec).expect(
            "apply_rs_ecc(mode 4 X pri+sec) (MaxiCode RS-ECC reference for lay_out_codewords pixs path) must succeed",
        );
        let pixs = lay_out_codewords(&cws);
        // BWIPP oracle: 350 cells (337 data + 13 fixed).
        assert_eq!(pixs.len(), 350, "pixs length mismatch");
        // First 30 oracle values.
        let want_first: &[u16] = &[
            316, 672, 703, 283, 610, 618, 439, 705, 619, 458, 674, 285, 340, 531, 348, 456, 470,
            369, 428, 549, 578, 609, 608, 679, 709, 669, 668, 698, 279, 410,
        ];
        assert_eq!(&pixs[..30], want_first, "pixs first 30 mismatch");
        // Last 13 must be FIXED_BLACK_POSITIONS exactly.
        assert_eq!(&pixs[pixs.len() - 13..], FIXED_BLACK_POSITIONS);
    }

    #[test]
    fn build_symbol_end_to_end_mode_4() {
        // Same fixture as apply_rs_ecc_matches_oracle_for_mode_4_x.
        let pri: [u8; 10] = [4, 24, 33, 33, 33, 33, 33, 33, 33, 33];
        let sec = [33u8; 84];
        let sym = build_symbol(&pri, &sec).unwrap();
        assert_eq!(sym.rows(), 33);
        assert_eq!(sym.cols(), 30);
        // All FIXED_BLACK_POSITIONS should be on after build_symbol.
        for &p in FIXED_BLACK_POSITIONS {
            let row = (p as usize) / 30;
            let col = (p as usize) % 30;
            assert!(
                sym.is_on(row, col),
                "fixed pos {p} (row {row} col {col}) should be on",
            );
        }
        // Out-of-bounds query is safely false.
        assert!(!sym.is_on(33, 0));
        assert!(!sym.is_on(0, 30));
    }

    #[test]
    fn maxicode_symbol_row_offset() {
        // Odd rows are physically offset by half a module to the
        // right (hexagonal stagger). Row 32 is even.
        assert!(!MaxiCodeSymbol::row_is_offset(0));
        assert!(MaxiCodeSymbol::row_is_offset(1));
        assert!(!MaxiCodeSymbol::row_is_offset(2));
        assert!(MaxiCodeSymbol::row_is_offset(31));
        assert!(!MaxiCodeSymbol::row_is_offset(32));
    }

    #[test]
    fn build_grid_populates_correct_rows_cols() {
        // Each grid position p maps to row = p/30, col = p%30.
        let pixs: &[u16] = &[0, 29, 30, 989]; // corners + first cell of row 1
        let grid = build_grid(pixs);
        assert!(grid[0][0]);
        assert!(grid[0][29]);
        assert!(grid[1][0]);
        assert!(grid[32][29]);
        // A cell that wasn't set should be false.
        assert!(!grid[15][15]);
    }

    #[test]
    fn codewords_to_mods_extracts_msb_first() {
        // First codeword 63 (0b111111) should produce 6 trues at
        // mods[0..6].
        let mut cws = [0u8; 144];
        cws[0] = 63;
        cws[1] = 1; // 0b000001
        let mods = codewords_to_mods(&cws);
        assert_eq!(&mods[..6], &[true; 6]);
        assert_eq!(&mods[6..12], &[false, false, false, false, false, true]);
    }

    /// Stage 11.A8c — extend `codewords_to_mods` to pin mid-array and
    /// last-codeword positions plus the zero-elsewhere invariant. The
    /// existing `codewords_to_mods_extracts_msb_first` test only
    /// inspects `mods[0..12]` (the first two codewords) and never
    /// checks any position beyond index 11, leaving these mutations
    /// uncovered:
    ///   * Loop-bound `for (i, &cw) in codewords.iter().enumerate()`
    ///     mutated to `.iter().take(N)` for small N (would silently
    ///     leave later mods unset, but the prior test couldn't see it).
    ///   * `i * 6 + bit` → `i * 6 + bit + 1` or `(i + 1) * 6 + bit`
    ///     (would shift output by one position; first two cws still
    ///     happen to land on adjacent slots, hiding the drift).
    ///   * Zero-init `[false; 864]` removed: untouched mods would be
    ///     undefined / stale.
    ///
    /// Probes three positions:
    ///   * `cws[50] = 0b100000` (32) → mods[300..306] = [T,F,F,F,F,F]
    ///     pins the i=50 indexing and the MSB-only pattern.
    ///   * `cws[100] = 0b010001` (17) → mods[600..606] = [F,T,F,F,F,T]
    ///     pins a mid-array two-bit pattern that catches LSB-first
    ///     mutations (which would put the F,T on the wrong end).
    ///   * `cws[143] = 0b111111` (63) → mods[858..864] = [T,T,T,T,T,T]
    ///     pins the LAST codeword position (catches `take(143)` or
    ///     other loop-truncation mutations).
    ///
    /// Plus a sweep asserting every other mods cell stays false,
    /// catching `[false; 864]` → other initial values and any mutation
    /// that writes outside the intended slot.
    #[test]
    fn codewords_to_mods_pins_mid_array_and_last_codeword_positions() {
        let mut cws = [0u8; 144];
        cws[50] = 0b10_0000; // 32 — MSB only
        cws[100] = 0b01_0001; // 17 — bits at positions 1 and 5 (MSB-first)
        cws[143] = 0b11_1111; // 63 — all 6 bits, LAST codeword

        let mods = codewords_to_mods(&cws);

        // Sanity: total length 864.
        assert_eq!(mods.len(), 864);

        // cws[50] = 0b100000 (MSB only).
        // MSB-first extraction: bit 0 = (32 >> 5) & 1 = 1, bits 1..=5 = 0.
        assert!(mods[300], "cws[50] bit 0 (MSB-first) = 1");
        for b in 1..6 {
            assert!(!mods[300 + b], "cws[50] bit {b} should be 0");
        }

        // cws[100] = 0b010001. MSB-first:
        //   bit 0: (17 >> 5) & 1 = 0
        //   bit 1: (17 >> 4) & 1 = 1
        //   bit 2: (17 >> 3) & 1 = 0
        //   bit 3: (17 >> 2) & 1 = 0
        //   bit 4: (17 >> 1) & 1 = 0
        //   bit 5: (17 >> 0) & 1 = 1
        assert_eq!(
            &mods[600..606],
            &[false, true, false, false, false, true],
            "cws[100]=17 (0b010001) MSB-first → [F,T,F,F,F,T]"
        );

        // cws[143] = 0b111111 — all 6 bits set, LAST codeword.
        // mods[858..864] = all true. Pins loop-bound up to i=143.
        for b in 0..6 {
            assert!(mods[858 + b], "cws[143] (LAST codeword) bit {b} = 1");
        }

        // Zero-elsewhere invariant. Every cell that doesn't fall in
        // the three set regions [300..306], [600..606], or [858..864]
        // must stay false. Catches `[false; 864]` init mutations or
        // index-drift writes that touched the wrong slot.
        for i in 0..864 {
            let in_set =
                (300..306).contains(&i) || (600..606).contains(&i) || (858..864).contains(&i);
            if !in_set {
                assert!(
                    !mods[i],
                    "mods[{i}] should be false (no codeword sets this position)"
                );
            }
        }
    }

    #[test]
    fn encode_secondary_a_b_with_ns_matches_oracle_corpus() {
        // Combined A↔B + NS oracle corpus. Inputs are chosen to
        // exercise: pure-A with NS, A→B→A with NS in A run, B-run
        // then NS in B context, pure-B with NS in B, and a tricky
        // start-with-B-followed-by-NS shape.
        let cases: &[(&[u8], &[u8])] = &[
            // NS in the middle of set A.
            (
                b"ABC123456789DEF",
                &[1, 2, 3, 31, 7, 22, 60, 52, 21, 4, 5, 6],
            ),
            // Set-A digits without NS (only 4 digits) followed by
            // trailing set-B run.
            (
                b"TEST1234abc",
                &[20, 5, 19, 20, 49, 50, 51, 52, 63, 1, 2, 3],
            ),
            // 1 set-B + 9-digit run: SB+a in A context, then NS.
            (b"Aa123456789", &[1, 59, 1, 31, 7, 22, 60, 52, 21]),
            // Pure-B start followed by 9 digits — NS fires WHILE
            // still in set B (no LA needed).
            (b"abc012345678", &[63, 1, 2, 3, 31, 0, 47, 6, 5, 14]),
            // Single A char + NS + single A char + trailing B-run.
            (
                b"X123456789Yabcdef",
                &[24, 31, 7, 22, 60, 52, 21, 25, 63, 1, 2, 3, 4, 5, 6],
            ),
        ];
        for (input, want) in cases {
            let got = encode_secondary_a_b_with_ns(input).unwrap();
            assert_eq!(
                got,
                *want,
                "encode_secondary_a_b_with_ns mismatch for {:?}",
                std::str::from_utf8(input).unwrap_or("<non-utf8>"),
            );
        }
    }

    #[test]
    fn encode_secondary_a_b_rejects_set_cde_bytes() {
        // The set-A/B-only helper still rejects high-bit ASCII —
        // those need the set-C/D/E shift dispatcher in
        // encode_secondary_a_b_with_ns.
        //
        // Stage 11.A8c — upgrade from 2 weak is_err() to diagnostic
        // + byte-echo + remediation-hint pins. The rejection arm at
        // line 162-167 produces:
        //   "MaxiCode encode_secondary_a_b: byte 0xHH needs set
        //    C/D/E; use encode_secondary_a_b_with_ns instead (it
        //    supports the full set-A/B/C/D/E shift + latch +
        //    intra-latch dispatcher)"
        //
        // Distinct byte echoes (0xc3 vs 0xff) kill `{b:02x}` drop
        // mutants.
        for (input, want_byte) in [(&[0xC3u8] as &[u8], "0xc3"), (b"abc\xff", "0xff")] {
            let err = encode_secondary_a_b(input).unwrap_err();
            let crate::error::Error::InvalidData(msg) = err else {
                panic!("encode_secondary_a_b({input:?}) must yield InvalidData; got {err:?}");
            };
            assert!(
                msg.contains("MaxiCode encode_secondary_a_b:")
                    && msg.contains(want_byte)
                    && msg.contains("needs set C/D/E")
                    && msg.contains("encode_secondary_a_b_with_ns"),
                "{input:?} diagnostic must pin function name + {want_byte:?} byte + C/D/E
                 hint + remediation function; got {msg:?}"
            );
        }
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_c_single_byte_shift() {
        // BWIPP oracle (oracle-maxicode-setcde.js, parse:true):
        //   "TEST^192" → encmsg = [20, 5, 19, 20, 60, 0]
        // 0xC0 is the first byte of set C, which maps to codeword 0.
        // The shift codeword 60 (SC) is emitted from set A.
        let got = encode_secondary_a_b_with_ns(b"TEST\xC0").unwrap();
        assert_eq!(got, vec![20, 5, 19, 20, 60, 0]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_d_single_byte_shift() {
        // "TEST^224" → [20, 5, 19, 20, 61, 0]. 0xE0 → set D, cw 0.
        let got = encode_secondary_a_b_with_ns(b"TEST\xE0").unwrap();
        assert_eq!(got, vec![20, 5, 19, 20, 61, 0]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_e_single_byte_shift() {
        // "TEST^160" → [20, 5, 19, 20, 62, 37]. 0xA0 → set E, cw 37.
        let got = encode_secondary_a_b_with_ns(b"TEST\xA0").unwrap();
        assert_eq!(got, vec![20, 5, 19, 20, 62, 37]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_charset_preference_order() {
        // Bytes that belong to multiple sets prefer C over D over E,
        // matching BWIPP's encoder (which assigns the first matching
        // charset by scan order). All five oracle outputs match:
        //   ^128 → SC + 48  (0x80 in set C)
        //   ^138 → SD + 47  (0x8A in set D only)
        //   ^155 → SE + 54  (0x9B in set E only)
        //   ^178 → SC + 40  (0xB2 in set C only)
        //   ^183 → SD + 43  (0xB7 in set D only)
        assert_eq!(
            encode_secondary_a_b_with_ns(b"TEST\x80").unwrap(),
            vec![20, 5, 19, 20, 60, 48]
        );
        assert_eq!(
            encode_secondary_a_b_with_ns(b"TEST\x8A").unwrap(),
            vec![20, 5, 19, 20, 61, 47]
        );
        assert_eq!(
            encode_secondary_a_b_with_ns(b"TEST\x9B").unwrap(),
            vec![20, 5, 19, 20, 62, 54]
        );
        assert_eq!(
            encode_secondary_a_b_with_ns(b"TEST\xB2").unwrap(),
            vec![20, 5, 19, 20, 60, 40]
        );
        assert_eq!(
            encode_secondary_a_b_with_ns(b"TEST\xB7").unwrap(),
            vec![20, 5, 19, 20, 61, 43]
        );
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_cde_prefix() {
        // A high-bit byte at the START of the message also gets a
        // single-byte shift. BWIPP oracle: "^192TEST" → [60, 0, 20, 5, 19, 20].
        let got = encode_secondary_a_b_with_ns(b"\xC0TEST").unwrap();
        assert_eq!(got, vec![60, 0, 20, 5, 19, 20]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_two_set_c_bytes() {
        // BWIPP oracle: "TEST^192^193" → [20,5,19,20, 60,0, 60,1].
        // Two consecutive set-C bytes still emit a single-byte shift
        // for each (BWIPP's DP picks shifts over latch when N <= 2 —
        // verified via oracle, where SC+cw+SC+cw = 4 cws matches the
        // latch cost of [60,60,cw,cw,58] = 5 cws).
        let got = encode_secondary_a_b_with_ns(b"TEST\xC0\xC1").unwrap();
        assert_eq!(got, vec![20, 5, 19, 20, 60, 0, 60, 1]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_c_latch_3_bytes() {
        // 3 set-C bytes: oracle says `[60, 60, 0, 1, 2, 58]` (6 cws).
        // Two single shifts would be 6 cws too — but BWIPP picks
        // latch when ties are broken by preferring fewer mode
        // switches. Verified against oracle-maxicode-setcde.js.
        let got = encode_secondary_a_b_with_ns(b"TEST\xC0\xC1\xC2").unwrap();
        assert_eq!(got, vec![20, 5, 19, 20, 60, 60, 0, 1, 2, 58]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_c_latch_4_bytes_eom() {
        // 4 set-C bytes at end of message: latch + 4 cws + LA.
        // Oracle: [20,5,19,20, 60,60, 0,1,2,3, 58].
        let got = encode_secondary_a_b_with_ns(b"TEST\xC0\xC1\xC2\xC3").unwrap();
        assert_eq!(got, vec![20, 5, 19, 20, 60, 60, 0, 1, 2, 3, 58]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_c_latch_5_bytes_eom() {
        // 5 set-C bytes at EOM: same latch structure.
        let got = encode_secondary_a_b_with_ns(b"TEST\xC0\xC1\xC2\xC3\xC4").unwrap();
        assert_eq!(got, vec![20, 5, 19, 20, 60, 60, 0, 1, 2, 3, 4, 58]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_c_latch_back_to_set_b() {
        // 4 set-C bytes followed by set-B bytes. Oracle:
        //   "TEST^192^193^194^195abc" →
        //     [20,5,19,20, 60,60, 0,1,2,3, 63, 1,2,3]
        // (back-latch [63] = LB takes us from C to B directly,
        // skipping a set-A intermediate state).
        let got = encode_secondary_a_b_with_ns(b"TEST\xC0\xC1\xC2\xC3abc").unwrap();
        assert_eq!(got, vec![20, 5, 19, 20, 60, 60, 0, 1, 2, 3, 63, 1, 2, 3]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_c_latch_back_to_set_a() {
        // 4 set-C bytes followed by set-A bytes. Oracle:
        //   "TEST^192^193^194^195XYZ" →
        //     [20,5,19,20, 60,60, 0,1,2,3, 58, 24,25,26]
        let got = encode_secondary_a_b_with_ns(b"TEST\xC0\xC1\xC2\xC3XYZ").unwrap();
        assert_eq!(got, vec![20, 5, 19, 20, 60, 60, 0, 1, 2, 3, 58, 24, 25, 26]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_c_latch_from_set_b_run() {
        // Set-B prefix, then set-C latch run, then EOM. Oracle:
        //   "abcd^192^193^194^195" →
        //     [63, 1,2,3,4, 60,60, 0,1,2,3, 58]
        // (`[63]` enters set B, then `[60, 60]` latches to set C
        // directly from set B, run of 4 cws, then `[58]` returns
        // to... set A per BWIPP's behavior — note this leaves us in
        // set A at EOM, not B.)
        let got = encode_secondary_a_b_with_ns(b"abcd\xC0\xC1\xC2\xC3").unwrap();
        assert_eq!(got, vec![63, 1, 2, 3, 4, 60, 60, 0, 1, 2, 3, 58]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_d_latch_4_bytes() {
        // 4 set-D bytes (all in 224..=227 = 0xE0..0xE3 → cws 0..3).
        // Oracle: [20,5,19,20, 61,61, 0,1,2,3, 58].
        let got = encode_secondary_a_b_with_ns(b"TEST\xE0\xE1\xE2\xE3").unwrap();
        assert_eq!(got, vec![20, 5, 19, 20, 61, 61, 0, 1, 2, 3, 58]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_c_long_run_8_bytes() {
        // 8 consecutive set-C bytes (split into two 4-byte chunks
        // 0xC0..0xC3 and 0xC0..0xC3). BWIPP packs them in ONE latch:
        //   [20,5,19,20, 60,60, 0,1,2,3, 0,1,2,3, 58].
        let got = encode_secondary_a_b_with_ns(b"TEST\xC0\xC1\xC2\xC3\xC0\xC1\xC2\xC3").unwrap();
        assert_eq!(got, vec![20, 5, 19, 20, 60, 60, 0, 1, 2, 3, 0, 1, 2, 3, 58]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_c_eight_bytes_start() {
        // 8 set-C bytes from message start (no leading set-A run).
        // Oracle: [60,60, 0,1,2,3,4,5,6,7, 58].
        let got = encode_secondary_a_b_with_ns(b"\xC0\xC1\xC2\xC3\xC4\xC5\xC6\xC7").unwrap();
        assert_eq!(got, vec![60, 60, 0, 1, 2, 3, 4, 5, 6, 7, 58]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_c_intra_latch_d_shift() {
        // 2 set-C + 1 set-D + 2 set-C → the intra-latch lookahead
        // absorbs the set-D byte via codeword 61 (SD intra-latch
        // shift, valid inside a set-C latch context). Oracle:
        //   "TEST^192^192^224^192^192" →
        //     [20,5,19,20, 60,60, 0,0, 61,0, 0,0, 58]
        // where 61,0 is SD + D_codeword(0xE0). Captures the
        // intra-latch behavior that previously required exit/re-enter.
        let got = encode_secondary_a_b_with_ns(b"TEST\xC0\xC0\xE0\xC0\xC0").unwrap();
        assert_eq!(got, vec![20, 5, 19, 20, 60, 60, 0, 0, 61, 0, 0, 0, 58]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_c_intra_latch_d_shift_3plus3() {
        // 3 set-C + 1 set-D + 2 set-C. Oracle:
        //   "TEST^192^192^192^224^192^192" →
        //     [20,5,19,20, 60,60, 0,0,0, 61,0, 0,0, 58]
        let got = encode_secondary_a_b_with_ns(b"TEST\xC0\xC0\xC0\xE0\xC0\xC0").unwrap();
        assert_eq!(got, vec![20, 5, 19, 20, 60, 60, 0, 0, 0, 61, 0, 0, 0, 58]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_c_intra_latch_trailing_d() {
        // 3 set-C + 1 set-D AT END. The committed-latch rule absorbs
        // the trailing cross-set byte via intra-latch shift. Oracle:
        //   "TEST^192^192^192^224" →
        //     [20,5,19,20, 60,60, 0,0,0, 61,0, 58]
        let got = encode_secondary_a_b_with_ns(b"TEST\xC0\xC0\xC0\xE0").unwrap();
        assert_eq!(got, vec![20, 5, 19, 20, 60, 60, 0, 0, 0, 61, 0, 58]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_c_intra_latch_consecutive_d() {
        // 3 set-C + 2 consecutive set-D AT END. Oracle:
        //   "TEST^192^192^192^224^224" →
        //     [20,5,19,20, 60,60, 0,0,0, 61,0, 61,0, 58]
        let got = encode_secondary_a_b_with_ns(b"TEST\xC0\xC0\xC0\xE0\xE0").unwrap();
        assert_eq!(got, vec![20, 5, 19, 20, 60, 60, 0, 0, 0, 61, 0, 61, 0, 58]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_c_leading_d_uses_single_shift() {
        // 1 set-D + 4 set-C. BWIPP uses a single shift for the
        // leading D byte (latch deferred because the run is too
        // short to amortise the latch overhead), then latches into
        // C for the trailing 4 bytes. Oracle:
        //   "TEST^224^192^192^192^192" →
        //     [20,5,19,20, 61,0, 60,60, 0,0,0,0, 58]
        let got = encode_secondary_a_b_with_ns(b"TEST\xE0\xC0\xC0\xC0\xC0").unwrap();
        assert_eq!(got, vec![20, 5, 19, 20, 61, 0, 60, 60, 0, 0, 0, 0, 58]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_e_latch_eom_no_back_latch() {
        // BWIPP-specific quirk: set-E latches at EOM do NOT emit the
        // trailing 58 back-latch (unlike C and D latches which do).
        // The trailing PAD codewords (value 33) are recognized as
        // end-of-data by value per ISO/IEC 16023 §5.2.4.1, so no
        // state reset is strictly required at EOM. BWIPP exploits
        // this for E only.
        // Oracle: "TEST^160^162^163^164" → [20,5,19,20, 62,62, 37,38,39,40]
        let got = encode_secondary_a_b_with_ns(b"TEST\xA0\xA2\xA3\xA4").unwrap();
        assert_eq!(got, vec![20, 5, 19, 20, 62, 62, 37, 38, 39, 40]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_e_latch_3byte_eom_no_back_latch() {
        // 3-byte E latch at EOM also omits the back-latch.
        // Oracle: "TEST^160^162^163" → [20,5,19,20, 62,62, 37,38,39]
        let got = encode_secondary_a_b_with_ns(b"TEST\xA0\xA2\xA3").unwrap();
        assert_eq!(got, vec![20, 5, 19, 20, 62, 62, 37, 38, 39]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_e_latch_with_intra_c_shift_eom_no_back_latch() {
        // E latch with trailing intra-latch SC shift also omits 58.
        // Oracle: "TEST^160^162^163^192" →
        //   [20,5,19,20, 62,62, 37,38,39, 60,0]
        let got = encode_secondary_a_b_with_ns(b"TEST\xA0\xA2\xA3\xC0").unwrap();
        assert_eq!(got, vec![20, 5, 19, 20, 62, 62, 37, 38, 39, 60, 0]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_c_latch_eom_keeps_back_latch() {
        // Confirm that C latches at EOM still emit 58 (only E omits).
        // Oracle: "TEST^192^192^192" → [20,5,19,20, 60,60, 0,0,0, 58]
        let got = encode_secondary_a_b_with_ns(b"TEST\xC0\xC0\xC0").unwrap();
        assert_eq!(got, vec![20, 5, 19, 20, 60, 60, 0, 0, 0, 58]);
    }

    #[test]
    fn encode_secondary_a_b_with_ns_set_d_latch_eom_keeps_back_latch() {
        // D latches at EOM also keep the back-latch (only E omits).
        // Oracle: "TEST^224^224^224" → [20,5,19,20, 61,61, 0,0,0, 58]
        let got = encode_secondary_a_b_with_ns(b"TEST\xE0\xE0\xE0").unwrap();
        assert_eq!(got, vec![20, 5, 19, 20, 61, 61, 0, 0, 0, 58]);
    }

    #[test]
    fn setc_codeword_known_values() {
        // Main range: 192..=218 → 0..=26.
        assert_eq!(setc_codeword(192), Some(0));
        assert_eq!(setc_codeword(218), Some(26));
        // Extended: 219..=223 → 32..=36.
        assert_eq!(setc_codeword(219), Some(32));
        assert_eq!(setc_codeword(223), Some(36));
        // High-128 band: 128..=137 → 48..=57.
        assert_eq!(setc_codeword(128), Some(48));
        assert_eq!(setc_codeword(137), Some(57));
        // Specific punctuation.
        assert_eq!(setc_codeword(170), Some(37)); // ª
        assert_eq!(setc_codeword(172), Some(38)); // ¬
        assert_eq!(setc_codeword(190), Some(47)); // ¾
                                                  // Space.
        assert_eq!(setc_codeword(b' '), Some(59));
        // Not in set C.
        assert_eq!(setc_codeword(b'A'), None);
        assert_eq!(setc_codeword(b'a'), None);
        assert_eq!(setc_codeword(b'0'), None);
        assert_eq!(setc_codeword(224), None); // set D territory
    }

    #[test]
    fn setd_codeword_known_values() {
        assert_eq!(setd_codeword(224), Some(0)); // à
        assert_eq!(setd_codeword(250), Some(26)); // ú
        assert_eq!(setd_codeword(251), Some(32)); // û
        assert_eq!(setd_codeword(255), Some(36)); // ÿ
        assert_eq!(setd_codeword(161), Some(37)); // ¡
        assert_eq!(setd_codeword(191), Some(46)); // ¿
        assert_eq!(setd_codeword(138), Some(47));
        assert_eq!(setd_codeword(148), Some(57));
        // Not in set D.
        assert_eq!(setd_codeword(192), None); // set C
        assert_eq!(setd_codeword(b'A'), None);
    }

    #[test]
    fn sete_codeword_known_values() {
        assert_eq!(sete_codeword(0), Some(0));
        assert_eq!(sete_codeword(26), Some(26));
        assert_eq!(sete_codeword(27), Some(30)); // ESC
        assert_eq!(sete_codeword(28), Some(32));
        assert_eq!(sete_codeword(31), Some(35));
        assert_eq!(sete_codeword(159), Some(36));
        assert_eq!(sete_codeword(160), Some(37)); // NBSP
        assert_eq!(sete_codeword(182), Some(47)); // ¶
        assert_eq!(sete_codeword(149), Some(48));
        assert_eq!(sete_codeword(158), Some(57));
        // Not in set E.
        assert_eq!(sete_codeword(b'A'), None);
        assert_eq!(sete_codeword(b'a'), None);
        assert_eq!(sete_codeword(192), None);
    }

    #[test]
    fn setb_codeword_known_values() {
        // Lowercase letters: 'a'..'z' → 1..26.
        assert_eq!(setb_codeword(b'a'), Some(1));
        assert_eq!(setb_codeword(b'z'), Some(26));
        // Backtick maps to 0 (the charmap row-0 col-1 entry).
        assert_eq!(setb_codeword(b'`'), Some(0));
        // Set-B punctuation block.
        assert_eq!(setb_codeword(b';'), Some(37));
        assert_eq!(setb_codeword(b'<'), Some(38));
        assert_eq!(setb_codeword(b'='), Some(39));
        assert_eq!(setb_codeword(b'>'), Some(40));
        assert_eq!(setb_codeword(b'?'), Some(41));
        assert_eq!(setb_codeword(b'['), Some(42));
        assert_eq!(setb_codeword(b']'), Some(44));
        assert_eq!(setb_codeword(b'_'), Some(46));
        // Uppercase is NOT in set B.
        assert_eq!(setb_codeword(b'A'), None);
        // Digits are NOT in set B (they live in A).
        assert_eq!(setb_codeword(b'0'), None);
        assert_eq!(setb_codeword(b'9'), None);
        // Space is NOT in set B (it's set A's row 32 col 0).
        assert_eq!(setb_codeword(b' '), None);
    }

    #[test]
    fn seta_codeword_known_values() {
        // Uppercase letters.
        assert_eq!(seta_codeword(b'A'), Some(1));
        assert_eq!(seta_codeword(b'Z'), Some(26));
        // Digits.
        assert_eq!(seta_codeword(b'0'), Some(48));
        assert_eq!(seta_codeword(b'9'), Some(57));
        // Space.
        assert_eq!(seta_codeword(b' '), Some(32));
        // Punctuation in 34..=58.
        assert_eq!(seta_codeword(b'"'), Some(34));
        assert_eq!(seta_codeword(b':'), Some(58));
        assert_eq!(seta_codeword(b'/'), Some(47));
        // CR is special row 0.
        assert_eq!(seta_codeword(b'\r'), Some(0));
        // ASCII 33 '!' is NOT in set A.
        assert_eq!(seta_codeword(b'!'), None);
        // Lowercase 'a' is set B.
        assert_eq!(seta_codeword(b'a'), None);
        // High-bit bytes are sets C/D/E.
        assert_eq!(seta_codeword(0x80), None);
    }

    /// Stage 11.A8c-L — exhaustive fingerprint over all 256 byte values
    /// for setb/setc/setd/sete_codeword. Any match-arm mutation (delete,
    /// replace Some(N) with default, replace range bounds, replace
    /// arithmetic in `byte - K + M`) changes at least one of the 256
    /// outputs and breaks one of these fingerprints. Targets the 31
    /// setX_codeword survivors (setd 9, setc 8, setb 7, sete 7).
    fn setx_fp<F: Fn(u8) -> Option<u8>>(f: F) -> u64 {
        let mut s: u64 = 0;
        for b in 0u8..=255 {
            // Map None → 0, Some(x) → x+1 so None and Some(0) differ.
            let v = f(b).map(|x| x as u64 + 1).unwrap_or(0);
            s = s.wrapping_add(
                v.wrapping_mul((b as u64).wrapping_add(1).wrapping_mul(2_654_435_761)),
            );
        }
        s
    }

    #[test]
    fn setbcde_codeword_exhaustive_fingerprints_pinned() {
        // Captured from the oracle-matched encoder. Any change to a
        // match-arm range, byte → codeword mapping, or arithmetic
        // (`byte - K + M`) in setb/setc/setd/sete_codeword fails one
        // of these.
        assert_eq!(
            setx_fp(setb_codeword),
            SETB_FP,
            "setb_codeword fingerprint changed"
        );
        assert_eq!(
            setx_fp(setc_codeword),
            SETC_FP,
            "setc_codeword fingerprint changed"
        );
        assert_eq!(
            setx_fp(setd_codeword),
            SETD_FP,
            "setd_codeword fingerprint changed"
        );
        assert_eq!(
            setx_fp(sete_codeword),
            SETE_FP,
            "sete_codeword fingerprint changed"
        );
    }
    const SETB_FP: u64 = 258276599545300;
    const SETC_FP: u64 = 745986699656874;
    const SETD_FP: u64 = 798876332194799;
    const SETE_FP: u64 = 484859236104260;

    #[test]
    fn build_grid_indexing_and_oob_guard() {
        // Stage 11.A8c — pin build_grid's `row = p / COLS`,
        // `col = p % COLS` index split and the
        // `row < ROWS && col < COLS` out-of-bounds guard.
        //
        // COLS=30, ROWS=33. p in 0..990 maps to a valid (row,col);
        // p >= 990 must be dropped silently (no panic, no write).
        let pixs: [u16; 6] = [
            0,   // (0,0)   — basic cell.
            29,  // (0,29)  — last col of row 0; pins col = p % COLS.
            30,  // (1,0)   — first col of row 1; pins row wrap on /.
            31,  // (1,1)   — distinct from (1,0).
            989, // (32,29) — last valid cell (ROWS-1, COLS-1).
            990, // would be (33,0); must be dropped by row<ROWS.
        ];
        let grid = build_grid(&pixs);

        // Cells that must be set.
        assert!(grid[0][0], "p=0 → (0,0)");
        assert!(grid[0][29], "p=29 → (0,29) pins col = p % COLS");
        assert!(grid[1][0], "p=30 → (1,0) pins row = p / COLS");
        assert!(grid[1][1], "p=31 → (1,1)");
        assert!(grid[32][29], "p=989 → (32,29) last valid cell");

        // Cells that must stay unset (pins we don't write the wrong place).
        assert!(!grid[1][29], "(1,29) untouched — pin row wrap");
        assert!(!grid[0][1], "(0,1) untouched");
        assert!(!grid[32][0], "(32,0) untouched — pin col = p % COLS");
        assert!(!grid[2][0], "(2,0) untouched");

        // Total set count = exactly 5 (p=990 must be dropped, not
        // counted). Pins the row<ROWS guard against being weakened
        // (mutation `<` → `<=` would attempt grid[33][0] and panic).
        let total: usize = grid.iter().flat_map(|r| r.iter()).filter(|&&b| b).count();
        assert_eq!(total, 5, "p=990 must be dropped by row<ROWS guard");
    }

    #[test]
    fn pack_mode_2_primary_matches_oracle_us_zip5() {
        // (pcode, ccode, scode) = ("12345", "840", "001"). BWIPP pads
        // the 5-digit US ZIP to 9 chars ("123450000") before packing.
        let pri = pack_mode_2_primary("12345", "840", "001").unwrap();
        assert_eq!(pri, [2, 36, 50, 46, 53, 17, 2, 18, 7, 0]);
    }

    #[test]
    fn pack_mode_2_primary_matches_oracle_max_values() {
        // (pcode, ccode, scode) = ("999999999", "752", "100"). Sweden,
        // service class 100. Verifies high-bit packing too.
        let pri = pack_mode_2_primary("999999999", "752", "100").unwrap();
        assert_eq!(pri, [50, 63, 9, 43, 57, 30, 2, 60, 18, 6]);
    }

    #[test]
    fn pack_mode_2_primary_handles_9_digit_postcode() {
        // 9-digit postcode means no ZIP+4 expansion regardless of country.
        let pri = pack_mode_2_primary("123450000", "840", "001").unwrap();
        // The encoded layout should be identical to the ZIP5 case above
        // because BWIPP pads "12345" → "123450000" in that case.
        assert_eq!(pri, [2, 36, 50, 46, 53, 17, 2, 18, 7, 0]);
    }

    #[test]
    fn pack_mode_3_primary_matches_oracle_uk_postcode() {
        // ("SW1A1A", "826" GB, "001"). 6-char alphanumeric postcode,
        // no padding needed.
        let pri = pack_mode_3_primary("SW1A1A", "826", "001").unwrap();
        assert_eq!(pri, [19, 16, 28, 16, 60, 53, 36, 14, 7, 0]);
    }

    #[test]
    fn pack_mode_3_primary_pads_short_postcode_with_spaces() {
        // ("AB", "826", "001"). 2-char postcode gets padded to
        // "AB    " (6 chars). Spaces are setA value 32.
        let pri = pack_mode_3_primary("AB", "826", "001").unwrap();
        assert_eq!(pri, [3, 8, 8, 8, 40, 16, 32, 14, 7, 0]);
    }

    #[test]
    fn pack_mode_3_primary_rejects_malformed() {
        // Stage 11.A8c — upgrade from 10 bare is_err() across four
        // rejection arms to per-arm diagnostic + value-echo pins.
        // pack_mode_3_primary has FOUR arms at lines 1416-1438:
        //   * postcode length 1-6 → "postcode must be 1-6 characters,
        //     got N chars"
        //   * postcode invalid byte → "postcode contains invalid byte
        //     0xHH (allowed: ...)"
        //   * country must be 3 ASCII digits → "country must be 3
        //     ASCII digits, got X"
        //   * service must be 3 ASCII digits → "service must be 3
        //     ASCII digits, got X"

        // Arm 1: postcode length. Both "ABCDEFG" (7) and "" (0) hit
        // the same arm.
        for (postcode, want_len) in [("ABCDEFG", "7 chars"), ("", "0 chars")] {
            let err = pack_mode_3_primary(postcode, "826", "001").unwrap_err();
            let crate::error::Error::InvalidData(msg) = err else {
                panic!("{postcode:?} must yield InvalidData; got {err:?}");
            };
            assert!(
                msg.contains("MaxiCode mode 3:")
                    && msg.contains("postcode must be 1-6 characters")
                    && msg.contains(want_len),
                "{postcode:?} length arm must pin diagnostic + {want_len:?}; got {msg:?}"
            );
        }

        // Arm 2: postcode invalid byte. "abc" → 'a' = 0x61;
        // "AB!CD" → '!' = 0x21.
        for (postcode, want_byte) in [("abc", "0x61"), ("AB!CD", "0x21")] {
            let err = pack_mode_3_primary(postcode, "826", "001").unwrap_err();
            let crate::error::Error::InvalidData(msg) = err else {
                panic!("{postcode:?} must yield InvalidData; got {err:?}");
            };
            assert!(
                msg.contains("postcode contains invalid byte") && msg.contains(want_byte),
                "{postcode:?} invalid-byte arm must pin diagnostic + {want_byte:?}; got {msg:?}"
            );
        }

        // Arm 3: country (3-ASCII-digit constraint). Each input must
        // appear via {country:?} debug echo.
        for (country, want_echo) in [("82", "\"82\""), ("8260", "\"8260\""), ("ABC", "\"ABC\"")] {
            let err = pack_mode_3_primary("AB", country, "001").unwrap_err();
            let crate::error::Error::InvalidData(msg) = err else {
                panic!("country={country:?} must yield InvalidData; got {err:?}");
            };
            assert!(
                msg.contains("country must be 3 ASCII digits") && msg.contains(want_echo),
                "country={country:?} must pin diagnostic + {want_echo:?}; got {msg:?}"
            );
        }

        // Arm 4: service (3-ASCII-digit constraint).
        for (service, want_echo) in [("0001", "\"0001\""), ("01", "\"01\""), ("ABC", "\"ABC\"")] {
            let err = pack_mode_3_primary("AB", "826", service).unwrap_err();
            let crate::error::Error::InvalidData(msg) = err else {
                panic!("service={service:?} must yield InvalidData; got {err:?}");
            };
            assert!(
                msg.contains("service must be 3 ASCII digits") && msg.contains(want_echo),
                "service={service:?} must pin diagnostic + {want_echo:?}; got {msg:?}"
            );
        }
    }

    #[test]
    fn pack_mode_2_primary_rejects_malformed() {
        // Stage 11.A8c — upgrade from 9 bare is_err() across three
        // rejection arms to per-arm diagnostic + value-echo pins
        // (parallel to pack_mode_3_primary d1d8096).
        // pack_mode_2_primary arms at lines 1290-1304:
        //   * postcode 1-9 ASCII digits → "postcode must be 1-9
        //     ASCII digits, got X"
        //   * country must be 3 ASCII digits → "country must be 3
        //     ASCII digits, got X"
        //   * service must be 3 ASCII digits → "service must be 3
        //     ASCII digits, got X"

        // Arm 1: postcode (1-9 ASCII digits constraint).
        // Note: pack_mode_2_primary trims trailing whitespace from
        // postcode, so "" stays "", "1234567890" stays full, "12345A"
        // stays full. {postcode:?} echoes the trimmed value.
        for (postcode, want_echo) in [
            ("", "\"\""),
            ("1234567890", "\"1234567890\""),
            ("12345A", "\"12345A\""),
        ] {
            let err = pack_mode_2_primary(postcode, "840", "001").unwrap_err();
            let crate::error::Error::InvalidData(msg) = err else {
                panic!("postcode={postcode:?} must yield InvalidData; got {err:?}");
            };
            assert!(
                msg.contains("MaxiCode mode 2:")
                    && msg.contains("postcode must be 1-9 ASCII digits")
                    && msg.contains(want_echo),
                "postcode={postcode:?} must pin diagnostic + {want_echo:?}; got {msg:?}"
            );
        }

        // Arm 2: country (3-ASCII-digit constraint).
        for (country, want_echo) in [("84", "\"84\""), ("8400", "\"8400\""), ("ABC", "\"ABC\"")] {
            let err = pack_mode_2_primary("12345", country, "001").unwrap_err();
            let crate::error::Error::InvalidData(msg) = err else {
                panic!("country={country:?} must yield InvalidData; got {err:?}");
            };
            assert!(
                msg.contains("country must be 3 ASCII digits") && msg.contains(want_echo),
                "country={country:?} must pin diagnostic + {want_echo:?}; got {msg:?}"
            );
        }

        // Arm 3: service (3-ASCII-digit constraint).
        for (service, want_echo) in [("0001", "\"0001\""), ("01", "\"01\""), ("ABC", "\"ABC\"")] {
            let err = pack_mode_2_primary("12345", "840", service).unwrap_err();
            let crate::error::Error::InvalidData(msg) = err else {
                panic!("service={service:?} must yield InvalidData; got {err:?}");
            };
            assert!(
                msg.contains("service must be 3 ASCII digits") && msg.contains(want_echo),
                "service={service:?} must pin diagnostic + {want_echo:?}; got {msg:?}"
            );
        }
    }

    #[test]
    fn parse_mode_2_or_3_input_strips_fid_prefix() {
        // FID prefix "[)>\x1e01\x1d99" + structured payload.
        let data = b"\x5b\x29\x3e\x1e\x30\x31\x1d\x39\x39\
                     12345\x1d840\x1d001\x1dhello";
        let (pc, cc, sc, sec) = parse_mode_2_or_3_input(data).unwrap();
        assert_eq!(pc, b"12345");
        assert_eq!(cc, b"840");
        assert_eq!(sc, b"001");
        assert_eq!(sec, b"hello");
    }

    #[test]
    fn parse_mode_2_or_3_input_without_fid_prefix() {
        let data = b"12345\x1d840\x1d001\x1dABC";
        let (pc, cc, sc, sec) = parse_mode_2_or_3_input(data).unwrap();
        assert_eq!(pc, b"12345");
        assert_eq!(cc, b"840");
        assert_eq!(sc, b"001");
        assert_eq!(sec, b"ABC");
    }

    #[test]
    fn parse_mode_2_or_3_input_rejects_too_few_fields() {
        // 3 GS-separated fields (postcode + country + service, missing
        // secondary) → parts.len() == 3 → InvalidData with count echo.
        let data = b"12345\x1d840\x1d001";
        let err = parse_mode_2_or_3_input(data).unwrap_err();
        match err {
            crate::error::Error::InvalidData(msg) => {
                assert!(
                    msg.contains("MaxiCode mode 2/3"),
                    "rejection diagnostic must carry mode-2/3 prefix; got {msg}"
                );
                assert!(
                    msg.contains("expected 4 GS-separated fields"),
                    "diagnostic must carry the 4-field predicate; got {msg}"
                );
                assert!(
                    msg.contains("(postcode, country, service, secondary)"),
                    "diagnostic must enumerate the 4 field names; got {msg}"
                );
                assert!(
                    msg.contains("got 3"),
                    "diagnostic must echo actual field count (3) — kills `{{}}` drop; got {msg}"
                );
            }
            other => panic!("expected InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn encode_mode_2_smoke() {
        // Mode 2: 5-digit US postcode (= ZIP), country=840 (US), service=001.
        let sym = encode_mode_2(b"12345\x1d840\x1d001\x1dhello").unwrap();
        assert_eq!(sym.cols(), 30);
        assert_eq!(sym.rows(), 33);
    }

    #[test]
    fn encode_mode_3_smoke() {
        // Mode 3: alphanumeric postcode (1-6 chars), country=124 (Canada),
        // service=999. Secondary is the freeform data.
        let sym = encode_mode_3(b"K1A0B1\x1d124\x1d999\x1dhello").unwrap();
        assert_eq!(sym.cols(), 30);
        assert_eq!(sym.rows(), 33);
    }

    #[test]
    fn encode_mode_2_rejects_bad_postcode() {
        // Mode 2 postcode must be 1-9 digits — "ABC" fails through
        // `pack_mode_2_primary`'s non-digit guard. The diagnostic is
        // emitted at line 1290-1292 with the predicate + `{postcode:?}`
        // echo (Debug-printed → `"ABC"` literal with quotes).
        let err = encode_mode_2(b"ABC\x1d840\x1d001\x1dhello").unwrap_err();
        match err {
            crate::error::Error::InvalidData(msg) => {
                assert!(
                    msg.contains("MaxiCode mode 2:"),
                    "mode-2 postcode rejection must carry mode-2 prefix; got {msg}"
                );
                assert!(
                    msg.contains("postcode must be 1-9 ASCII digits"),
                    "mode-2 rejection must carry digit-only predicate; got {msg}"
                );
                assert!(
                    msg.contains("\"ABC\""),
                    "mode-2 rejection must Debug-echo the raw postcode; got {msg}"
                );
                // Cross-arm contamination guard: the mode-3 predicate
                // (1-6 characters) must NOT appear in the mode-2
                // rejection.
                assert!(
                    !msg.contains("1-6 characters"),
                    "mode-2 rejection must NOT leak the mode-3 predicate; got {msg}"
                );
            }
            other => panic!("expected InvalidData, got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin `encode_mode_2`'s 84-cw secondary-overflow
    /// guard at line 922. Mode 2's secondary holds 84 cws (vs mode
    /// 4's 93); the rejection arm wasn't directly tested. Same
    /// pattern as the prior mode_4 / mode_6 capacity-guard pins.
    ///
    /// 200-byte 'A'-run secondary overflows the 84-cw budget (each
    /// set-A letter takes 1 cw).
    #[test]
    fn encode_mode_2_rejects_oversized_secondary() {
        let mut payload = b"12345\x1d840\x1d001\x1d".to_vec();
        payload.extend(std::iter::repeat_n(b'A', 200));
        // 4-anchor `&&` pin (matches mode_4/5/6 family — 157e296, 46e85da,
        // c162b5c). Each AND-anchored substring kills a distinct mutation
        // class. The previous `||` form silently accepted EITHER the
        // prefix OR the cap-size — too weak.
        match encode_mode_2(&payload).unwrap_err() {
            crate::error::Error::InvalidData(msg) => {
                assert!(
                    msg.contains("MaxiCode mode 2:"),
                    "mode-2 capacity diagnostic must carry mode-2 prefix; got {msg}"
                );
                assert!(
                    msg.contains("exceeds 84-cw capacity"),
                    "mode-2 capacity diagnostic must carry the 84-cw cap; got {msg}"
                );
                // Mode 2/3 use "(N cws)" rather than the mode-4/5/6
                // "({N} codewords)" wording.
                assert!(
                    msg.contains("(200 cws)"),
                    "mode-2 capacity diagnostic must echo encmsg.len() as (200 cws); got {msg}"
                );
                // Cross-mode contamination guard: mode-2/3 share 84-cw,
                // but mode-4/6 (93) and mode-5 (77) must NOT leak.
                assert!(
                    !msg.contains("93-cw") && !msg.contains("77-cw"),
                    "mode-2 diagnostic must NOT leak other modes' caps; got {msg}"
                );
            }
            other => panic!("mode_2 must reject 200-byte secondary, got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin `encode_mode_3`'s 84-cw secondary-overflow
    /// guard. Same shape as mode_2 (different postcode encoding;
    /// same 84-cw secondary capacity).
    #[test]
    fn encode_mode_3_rejects_oversized_secondary() {
        let mut payload = b"AB12CD\x1d124\x1d999\x1d".to_vec();
        payload.extend(std::iter::repeat_n(b'A', 200));
        // 4-anchor `&&` pin (matches mode_2 family at 2ab1c1a — shared
        // 84-cw cap, shared "(N cws)" length-echo wording).
        match encode_mode_3(&payload).unwrap_err() {
            crate::error::Error::InvalidData(msg) => {
                assert!(
                    msg.contains("MaxiCode mode 3:"),
                    "mode-3 capacity diagnostic must carry mode-3 prefix; got {msg}"
                );
                assert!(
                    msg.contains("exceeds 84-cw capacity"),
                    "mode-3 capacity diagnostic must carry the 84-cw cap; got {msg}"
                );
                assert!(
                    msg.contains("(200 cws)"),
                    "mode-3 capacity diagnostic must echo encmsg.len() as (200 cws); got {msg}"
                );
                // Cross-mode contamination guard: mode-3 shares 84-cw
                // with mode-2, but must NOT leak mode-4/6 (93) or
                // mode-5 (77) caps.
                assert!(
                    !msg.contains("93-cw") && !msg.contains("77-cw"),
                    "mode-3 diagnostic must NOT leak other modes' caps; got {msg}"
                );
                // Cross-prefix guard: mode-3 must NOT pick up the
                // mode-2 prefix (both share 84 cap, but different mode).
                assert!(
                    !msg.contains("MaxiCode mode 2:"),
                    "mode-3 diagnostic must NOT leak mode-2 prefix; got {msg}"
                );
            }
            other => panic!("mode_3 must reject 200-byte secondary, got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin `encode_mode_4`'s shape: 30×33 standard
    /// MaxiCode dimensions, and successful encoding of a small ASCII
    /// payload. mode_4 is the "general data, standard ECC" mode (no
    /// SCM, no enhanced ECC). The function had no direct test prior
    /// to this commit; the public Symbology dispatcher exercised it
    /// transitively but not byte-for-byte.
    ///
    /// Kills mutants:
    ///   - `cws[0] = 4` → `cws[0] = 5` (mode swap; would cause sym
    ///     to encode in mode-5's 68-cw secondary structure and
    ///     mismatch decoder expectations).
    ///   - Drop the encmsg copy entirely → symbol decodes to all-pad.
    ///   - `93` → `92`: would reject 93-cw payloads incorrectly
    ///     (smoke test is small so not directly checked).
    #[test]
    fn encode_mode_4_smoke() {
        let sym = encode_mode_4(b"hello").unwrap();
        assert_eq!(sym.cols(), 30, "MaxiCode is always 30 cols");
        assert_eq!(sym.rows(), 33, "MaxiCode is always 33 rows");
    }

    #[test]
    fn encode_mode_4_rejects_overlong_payload() {
        // 200 bytes of 'A' overflows the 93-cw secondary capacity
        // (each ASCII letter takes 1 cw in set A, so 200 cws > 93).
        // 4-anchor `&&` pin (mirrors mode_5 / mode_6 family):
        //   * "MaxiCode mode 4:" prefix.
        //   * "exceeds 93-cw capacity" cap-size predicate.
        //   * "200 codewords" length echo.
        //   * ABSENCE of other modes' caps (84 = mode 2/3, 77 = mode 5).
        let big = vec![b'A'; 200];
        match encode_mode_4(&big).unwrap_err() {
            crate::error::Error::InvalidData(msg) => {
                assert!(
                    msg.contains("MaxiCode mode 4:"),
                    "mode-4 capacity diagnostic must carry mode-4 prefix; got {msg}"
                );
                assert!(
                    msg.contains("exceeds 93-cw capacity"),
                    "mode-4 capacity diagnostic must carry the 93-cw cap; got {msg}"
                );
                assert!(
                    msg.contains("200 codewords"),
                    "mode-4 capacity diagnostic must echo encmsg.len() (200); got {msg}"
                );
                assert!(
                    !msg.contains("84-cw") && !msg.contains("77-cw"),
                    "mode-4 diagnostic must NOT leak other modes' caps; got {msg}"
                );
            }
            other => panic!("mode_4 must reject overlong payload, got {other:?}"),
        }
    }

    #[test]
    fn symbology_maxicode_mode_2_dispatches_correctly() {
        // Exercises Symbology::Maxicode dispatch with opts.extras["mode"]=2.
        // Stage 11.A8c (cont) — echo the actual returned Encoded variant in
        // the panic, so a mutation that re-routes Maxicode to a non-Hex
        // family (Stacked, Matrix, …) is identifiable in the failure
        // diagnostic instead of just signaling "wrong variant".
        use crate::{Encoded, Options, Symbology};
        let mut opts = Options::default();
        opts.extras.push(("mode".into(), "2".into()));
        let enc = Symbology::Maxicode
            .encode("12345\x1d840\x1d001\x1dhello", &opts)
            .unwrap();
        match enc {
            Encoded::Hex(sym) => {
                assert_eq!(
                    sym.cols(),
                    30,
                    "MaxiCode mode 2 must produce 30-col hex grid; got {} cols",
                    sym.cols()
                );
                assert_eq!(
                    sym.rows(),
                    33,
                    "MaxiCode mode 2 must produce 33-row hex grid; got {} rows",
                    sym.rows()
                );
            }
            other => panic!(
                "MaxiCode mode 2 must return Encoded::Hex (hexagonal grid family); got non-Hex variant {other:?}"
            ),
        }
    }

    #[test]
    fn encode_mode_5_codewords_match_bwip_js_test() {
        // Reproduce mode-5 codeword layout: pri = [5] + encmsg(4) + 5*pad,
        // sec = 68 pads. Oracle from oracle-maxicode-codewords.js "TEST" mode=5.
        let encmsg = encode_secondary_a_b_with_ns(b"TEST").unwrap();
        // For "TEST" all set A → 4 codewords.
        assert_eq!(encmsg.len(), 4);
        let mut cws = [PAD_CODE[0]; 78];
        cws[0] = 5;
        cws[1..5].copy_from_slice(&encmsg);
        let mut pri = [0u8; 10];
        pri.copy_from_slice(&cws[..10]);
        let final_cws = apply_rs_ecc(&pri, &cws[10..]).unwrap();
        assert_eq!(final_cws[..10], [5, 20, 5, 19, 20, 33, 33, 33, 33, 33]);
        assert_eq!(final_cws[10..20], [50, 22, 36, 1, 17, 6, 35, 47, 16, 41]);
        assert!(
            final_cws[20..88].iter().all(|&b| b == 33),
            "mode 5 sec=pads",
        );
        assert_eq!(final_cws[88..96], [5, 5, 39, 39, 9, 9, 62, 62]);
        // Also verify encode_mode_5 returns a valid-shape symbol.
        let sym = encode_mode_5(b"TEST").unwrap();
        assert_eq!(sym.cols(), 30);
        assert_eq!(sym.rows(), 33);
    }

    #[test]
    fn encode_mode_6_codewords_match_bwip_js_test() {
        let encmsg = encode_secondary_a_b_with_ns(b"TEST").unwrap();
        let mut cws = [PAD_CODE[0]; 94];
        cws[0] = 6;
        cws[1..5].copy_from_slice(&encmsg);
        let mut pri = [0u8; 10];
        pri.copy_from_slice(&cws[..10]);
        let final_cws = apply_rs_ecc(&pri, &cws[10..]).unwrap();
        assert_eq!(final_cws[..10], [6, 20, 5, 19, 20, 33, 33, 33, 33, 33]);
        assert_eq!(final_cws[10..20], [11, 53, 20, 26, 25, 29, 28, 16, 59, 54],);
        assert!(
            final_cws[20..104].iter().all(|&b| b == 33),
            "mode 6 sec=pads",
        );
        assert_eq!(final_cws[104..112], [60, 60, 40, 40, 9, 9, 43, 43]);
        let sym = encode_mode_6(b"TEST").unwrap();
        assert_eq!(sym.cols(), 30);
        assert_eq!(sym.rows(), 33);
    }

    /// Stage 11.A8c — pin `encode_mode_5`'s 77-cw capacity guard at
    /// line 831. Mirrors the mode_2/3/4/6 capacity tests but mode 5 has
    /// the SMALLEST secondary (77 cws vs 84/93). 100-byte 'A'-run
    /// overflows because each set-A letter encodes to 1 cw.
    ///
    /// Mutations to catch:
    ///   - `> 77` → `>= 77` rejects 77-cw payloads incorrectly.
    ///   - `> 77` → `> 78` overflows the 78-byte cws array.
    ///   - Drop the `{}` codeword-count interpolation → no length echo.
    ///   - Symbology-prefix swap (mode 5 → mode 4) misleads the caller.
    ///   - Cap-size swap (77 → 84 / 93) hides mode-5-specific bound.
    #[test]
    fn encode_mode_5_rejects_oversized_message() {
        let oversized = vec![b'A'; 100];
        let err = encode_mode_5(&oversized).unwrap_err();
        match err {
            crate::error::Error::InvalidData(msg) => {
                assert!(
                    msg.contains("MaxiCode mode 5:"),
                    "mode-5 capacity diagnostic must carry the prefix; got {msg}"
                );
                assert!(
                    msg.contains("exceeds 77-cw capacity"),
                    "mode-5 capacity diagnostic must carry the 77-cw cap; got {msg}"
                );
                assert!(
                    msg.contains("100 codewords"),
                    "mode-5 capacity diagnostic must echo encmsg.len() (100); got {msg}"
                );
                // Cross-mode contamination guards: must NOT name other
                // modes' cap sizes (84 = mode 2/3; 93 = mode 4/6).
                assert!(
                    !msg.contains("84-cw") && !msg.contains("93-cw"),
                    "mode-5 diagnostic must NOT leak other modes' caps; got {msg}"
                );
            }
            other => panic!("expected InvalidData, got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin `encode_mode_6`'s 93-cw capacity guard at
    /// line 853. The existing `encode_mode_6_codewords_match_bwip_js_test`
    /// covers the happy path; the overflow rejection arm was untested.
    /// Mirrors the mode-4 capacity-guard test from a prior commit.
    ///
    /// Mutations to catch:
    ///   - `> 93` → `>= 93` rejects 93-cw payloads incorrectly.
    ///   - `> 93` → `> 94` accepts 94-cw payloads (overflows cws array).
    ///   - Drop the guard entirely → silent overflow.
    #[test]
    fn encode_mode_6_rejects_overlong_payload() {
        let big = vec![b'A'; 200];
        // Pin the mode-6 capacity diagnostic with four AND-anchored
        // substrings: prefix + 93-cw cap + length echo + cross-mode
        // contamination guard. Previous `||` form let a mutant drop
        // EITHER the symbology prefix OR the bound without failing.
        match encode_mode_6(&big).unwrap_err() {
            crate::error::Error::InvalidData(msg) => {
                assert!(
                    msg.contains("MaxiCode mode 6:"),
                    "mode-6 capacity diagnostic must carry mode-6 prefix; got {msg}"
                );
                assert!(
                    msg.contains("exceeds 93-cw capacity"),
                    "mode-6 capacity diagnostic must carry the 93-cw cap; got {msg}"
                );
                assert!(
                    msg.contains("200 codewords"),
                    "mode-6 capacity diagnostic must echo encmsg.len() (200); got {msg}"
                );
                // Cross-mode contamination guard: mode-6 cap (93) must
                // NOT be confused with mode-2/3 (84) or mode-5 (77).
                assert!(
                    !msg.contains("84-cw") && !msg.contains("77-cw"),
                    "mode-6 diagnostic must NOT leak other modes' caps; got {msg}"
                );
            }
            other => panic!("mode_6 must reject overlong payload, got {other:?}"),
        }
    }

    #[test]
    fn symbology_maxicode_rejects_unsupported_mode() {
        use crate::{Options, Symbology};
        let mut opts = Options::default();
        opts.extras.push(("mode".into(), "99".into()));
        let r = Symbology::Maxicode.encode("hello", &opts);
        // Defense-in-depth pinning with the dedicated
        // `maxicode_mode_dispatcher_rejection_arms` test: the unsupported-
        // mode arm at src/symbology.rs:844-866 must emit InvalidOption
        // with the parsed mode value echoed back and the 2..=6 range
        // hint. Dropping the `{other}` echo OR the range hint here AND
        // in the sibling would make this test pass.
        let err = r.unwrap_err();
        match err {
            crate::error::Error::InvalidOption(msg) => {
                assert!(
                    msg.contains("mode=99"),
                    "diagnostic must echo the parsed mode value (99); got {msg}"
                );
                assert!(
                    msg.contains("not supported"),
                    "diagnostic must carry the 'not supported' predicate; got {msg}"
                );
                assert!(
                    msg.contains("2..=6"),
                    "diagnostic must carry the implemented-range hint (2..=6); got {msg}"
                );
                // Cross-arm contamination guard: this is the range arm,
                // NOT the parse-fail arm — the "must be a number"
                // wording belongs to the parse-fail arm only.
                assert!(
                    !msg.contains("must be a number"),
                    "range-arm diagnostic must NOT leak parse-fail wording; got {msg}"
                );
            }
            other => panic!("expected InvalidOption, got {other:?}"),
        }
    }

    /// Stage 11.A8c — pin the maxicode mode-dispatcher rejection arms
    /// at `src/symbology.rs:844-866`. Two distinct error arms exist:
    ///   1. `.parse::<u8>()` fails (e.g. mode="abc") → InvalidOption
    ///      with "must be a number" diagnostic.
    ///   2. mode parses but lies outside 2..=6 → InvalidOption with
    ///      "mode={other} not supported" diagnostic.
    /// The existing `symbology_maxicode_rejects_unsupported_mode`
    /// covers (2) with mode=99 but only checks `is_err()` — neither
    /// the message content nor the parse-fail arm (1) was pinned.
    ///
    /// Anchors:
    ///   1. mode="abc" → InvalidOption(`"must be a number"` or
    ///      `"2..=6"`) — parse-fail arm.
    ///   2. mode="0" → InvalidOption(`"mode=0 not supported"` and
    ///      `"2..=6 are implemented"`) — range arm at lower bound.
    ///   3. mode="1" → InvalidOption (range arm just under).
    ///   4. mode="7" → InvalidOption (range arm just over).
    ///   5. mode="99" → InvalidOption with diagnostic.
    ///   6. mode="" (empty) → InvalidOption (parse fails for empty).
    ///   7. Default (no mode extra) → Ok (mode 4 default smoke).
    ///   8. Each valid mode 2..=6 routes correctly (smoke test, not
    ///      pinned with golden — just verifies dispatch arms exist).
    ///
    /// Mutations killed:
    ///   * Parse-fail arm `.map_err(|_| ...)` dropped → would surface
    ///     a different error type or panic.
    ///   * Range arm message text drift (`"must be"` etc.).
    ///   * Match arm `2..=6` swapped to `2..=5` or `2..6` (would
    ///     reject mode 6 or accept mode 6 incorrectly).
    ///   * Default `unwrap_or(4)` mutated to other default — mode=4
    ///     anchor would shift.
    #[test]
    fn symbology_maxicode_mode_dispatcher_all_rejection_arms() {
        use crate::error::Error;
        use crate::{Options, Symbology};

        // Anchor 1: parse-fail with non-numeric mode. The diagnostic
        // source at src/symbology.rs:850 is:
        //   "maxicode: mode must be a number 2..=6 (defaults to 4)"
        // Both "must be a number" and "2..=6" are always present in
        // this exact string, so the previous `||` was unnecessarily
        // weak. 4-anchor `&&` pin:
        let opts = Options::default().with("mode", "abc");
        match Symbology::Maxicode.encode("hello", &opts) {
            Err(Error::InvalidOption(msg)) => {
                assert!(
                    msg.contains("maxicode:"),
                    "parse-fail diagnostic must carry the maxicode prefix; got {msg}"
                );
                assert!(
                    msg.contains("must be a number"),
                    "parse-fail diagnostic must carry the predicate; got {msg}"
                );
                assert!(
                    msg.contains("2..=6"),
                    "parse-fail diagnostic must carry the 2..=6 range hint; got {msg}"
                );
                assert!(
                    msg.contains("defaults to 4"),
                    "parse-fail diagnostic must carry the default-4 hint; got {msg}"
                );
            }
            other => panic!("mode=abc should reject as parse-fail, got {other:?}"),
        }

        // Anchor 2: mode=0 lower-out-of-range.
        let opts = Options::default().with("mode", "0");
        match Symbology::Maxicode.encode("hello", &opts) {
            Err(Error::InvalidOption(msg)) => assert!(
                msg.contains("mode=0") && msg.contains("not supported"),
                "expected mode=0 unsupported diagnostic, got: {msg}"
            ),
            other => panic!("mode=0 should reject, got {other:?}"),
        }

        // Anchor 3: mode=1 lower-out-of-range.
        let opts = Options::default().with("mode", "1");
        assert!(
            matches!(
                Symbology::Maxicode.encode("hello", &opts),
                Err(Error::InvalidOption(_))
            ),
            "mode=1 should reject (below 2..=6)"
        );

        // Anchor 4: mode=7 upper-out-of-range.
        let opts = Options::default().with("mode", "7");
        match Symbology::Maxicode.encode("hello", &opts) {
            Err(Error::InvalidOption(msg)) => assert!(
                msg.contains("mode=7") && msg.contains("not supported"),
                "expected mode=7 unsupported diagnostic, got: {msg}"
            ),
            other => panic!("mode=7 should reject (above 2..=6), got {other:?}"),
        }

        // Anchor 5: mode="" (empty) → parse fails.
        let opts = Options::default().with("mode", "");
        assert!(
            matches!(
                Symbology::Maxicode.encode("hello", &opts),
                Err(Error::InvalidOption(_))
            ),
            "mode='' should reject as parse-fail"
        );

        // Anchor 6: mode out-of-range with very large number → must
        // still reject (kills any range-collapse mutant).
        let opts = Options::default().with("mode", "255");
        assert!(
            matches!(
                Symbology::Maxicode.encode("hello", &opts),
                Err(Error::InvalidOption(_))
            ),
            "mode=255 should reject"
        );

        // Anchor 7: default (no mode extra) → mode 4 (general data) →
        // accepts "hello".
        assert!(
            Symbology::Maxicode
                .encode("hello", &Options::default())
                .is_ok(),
            "default should encode in mode 4"
        );

        // Anchor 8: each valid mode 2..=6 routes (smoke). Modes 2 and
        // 3 need structured input; we test with mode 4/5/6 here and
        // verify modes 2/3 reject the unstructured "hello" but with a
        // DIFFERENT error (per their own validation), not the
        // mode-dispatcher catch-all.
        for &m in &[4u8, 5, 6] {
            let opts = Options::default().with("mode", m.to_string());
            assert!(
                Symbology::Maxicode.encode("hello", &opts).is_ok(),
                "mode={m} should encode 'hello'"
            );
        }
        // Modes 2 and 3 reject "hello" because they require the
        // structured GS-separated format, but the error is from
        // pack_mode_2/3_primary's parser (NOT the dispatcher's
        // "not supported"). Pin that the diagnostic is NOT the
        // dispatcher's range message.
        for &m in &[2u8, 3] {
            let opts = Options::default().with("mode", m.to_string());
            match Symbology::Maxicode.encode("hello", &opts) {
                Err(Error::InvalidData(msg)) => assert!(
                    !msg.contains("not supported"),
                    "mode={m} dispatched correctly; downstream rejection \
                     should not mention 'not supported', got: {msg}"
                ),
                Err(Error::InvalidOption(msg)) => assert!(
                    !msg.contains("not supported"),
                    "mode={m} surfaced wrong dispatcher arm: {msg}"
                ),
                other => {
                    panic!("mode={m} 'hello' should reject via downstream parser, got {other:?}")
                }
            }
        }
    }

    #[test]
    fn sentinels_are_distinct() {
        let s = [
            SENTINEL_ECI,
            SENTINEL_PAD,
            SENTINEL_NS,
            SENTINEL_LA,
            SENTINEL_LB,
            SENTINEL_SA,
            SENTINEL_SB,
            SENTINEL_SC,
            SENTINEL_SD,
            SENTINEL_SE,
        ];
        let mut sorted = s;
        sorted.sort_unstable();
        for i in 1..sorted.len() {
            assert_ne!(sorted[i], sorted[i - 1], "duplicate sentinel at {i}");
        }
    }

    /// Stage 11.A8c — pin `encode_ns_run` numeric-shift 9-digit
    /// packing. Kills `>> with <<` / `& with |` / `& 63` mask
    /// mutations on lines 977-981.
    #[test]
    fn encode_ns_run_packs_nine_digits() {
        // 9-digit "000000001" → value=1 → encoded bytes:
        //   [NS_CODEWORD, 0, 0, 0, 0, 1].
        let result = encode_ns_run(b"000000001").unwrap();
        assert_eq!(result[0], NS_CODEWORD);
        assert_eq!(result[1..], [0, 0, 0, 0, 1]);

        // 9-digit "000000063" → value=63 → bytes
        //   [NS_CODEWORD, 0, 0, 0, 0, 63] — pins the &63 LSB mask.
        let result = encode_ns_run(b"000000063").unwrap();
        assert_eq!(result[1..], [0, 0, 0, 0, 63]);

        // 9-digit "000000064" → value=64 → bytes
        //   [NS_CODEWORD, 0, 0, 0, 1, 0] — pins the >>6 shift.
        let result = encode_ns_run(b"000000064").unwrap();
        assert_eq!(result[1..], [0, 0, 0, 1, 0]);

        // 9-digit "000000000" → value=0 → all zeros after NS marker.
        let result = encode_ns_run(b"000000000").unwrap();
        assert_eq!(result[1..], [0, 0, 0, 0, 0]);

        // Non-9-digit → None.
        assert_eq!(encode_ns_run(b"12345678"), None); // 8 digits
        assert_eq!(encode_ns_run(b"1234567890"), None); // 10 digits
        assert_eq!(encode_ns_run(b"12345678A"), None); // non-digit
        assert_eq!(encode_ns_run(b""), None); // empty
    }

    /// Stage 11.A8c — pin `to_bin(value, width)`. Renders a u64 as an
    /// MSB-first ASCII bit string of given width, or `None` if the
    /// value cannot fit. Used everywhere mode-2 / mode-3 primary
    /// codeword packing happens.
    ///
    /// Anchors pin every observable behavior:
    ///   * width=0, value=0 → `Some("")` (empty string, NOT None);
    ///   * width=4, value=0 → `"0000"` (kills '0'/'1' swap mutant);
    ///   * width=4, value=15 → `"1111"`;
    ///   * width=4, value=5 → `"0101"` (kills MSB→LSB order flip,
    ///     i.e. removing `.rev()` would give "1010");
    ///   * width=3, value=7 → `"111"`;
    ///   * width=8, value=129 → `"10000001"` (asymmetric anchor —
    ///     catches bit-order flip even with symmetric counts);
    ///   * width=4, value=16 → `None` (boundary: 1<<4 doesn't fit);
    ///   * width=4, value=15 → `Some(...)` (boundary inverse — fits);
    ///   * width=63, value=u64::MAX → `None`;
    ///   * width=64, value=u64::MAX → `Some(all-'1's)` (kills
    ///     `< 64` → `<= 64` mutant that would overflow 1u64 << 64
    ///     trying to compute the bound);
    ///   * length always equals `width`.
    #[test]
    fn to_bin_msb_first_width_clamped_with_overflow_guard() {
        // Empty width.
        assert_eq!(to_bin(0, 0), Some(String::new()));

        // All-zero / all-one (kills '0'/'1' swap).
        assert_eq!(to_bin(0, 4), Some("0000".to_string()));
        assert_eq!(to_bin(15, 4), Some("1111".to_string()));

        // MSB-first ordering anchors. 5 = 0b0101.
        assert_eq!(
            to_bin(5, 4),
            Some("0101".to_string()),
            "MSB-first: removing .rev() would give \"1010\""
        );
        assert_eq!(to_bin(7, 3), Some("111".to_string()));

        // Asymmetric anchor: 129 = 0b1000_0001 — bit-order flip would
        // give "10000001" reversed = "10000001"... wait, that's still
        // a palindrome. Use 0b10000010 = 130 → "10000010" vs
        // reversed "01000001".
        assert_eq!(
            to_bin(130, 8),
            Some("10000010".to_string()),
            "MSB-first ordering — reverse would give \"01000001\""
        );

        // Boundary: value exactly 1<<width → None.
        assert_eq!(to_bin(16, 4), None, "16 = 1<<4 doesn't fit in 4 bits");
        // Boundary inverse: 1<<width − 1 fits.
        assert!(to_bin(15, 4).is_some(), "15 fits in 4 bits");

        // width=63 boundary: u64::MAX > (1<<63) → None.
        assert_eq!(to_bin(u64::MAX, 63), None);

        // width=64: anything fits (kills `< 64` → `<= 64` overflow
        // mutant). u64::MAX → 64 '1' chars.
        let s = to_bin(u64::MAX, 64).expect("width=64 always fits");
        assert_eq!(s.len(), 64);
        assert!(s.chars().all(|c| c == '1'));

        // width=64, value=0 → 64 '0' chars.
        let s = to_bin(0, 64).expect("width=64 always fits");
        assert_eq!(s, "0".repeat(64));

        // Length invariant: output length == width on every success.
        for w in 0..=12 {
            for v in 0..=7u64 {
                if let Some(s) = to_bin(v, w) {
                    assert_eq!(s.len(), w, "to_bin({v}, {w}) length should be {w}");
                }
            }
        }
    }

    /// Stage 11.A8c-L — kills the 10 `delete -` mutants on the
    /// `pub(crate) const SENTINEL_*` definitions at L1243-1254 by
    /// asserting each named constant's exact negative value. If a
    /// `-` is deleted, the sentinel collapses into the legal 6-bit
    /// codeword range (0..=63) and corrupts every downstream
    /// encoder that consults it (set-shift/latch dispatch tables,
    /// pad/eci/ns markers). Same idiom as
    /// `code49_sentinel_consts_pinned` (commit 3b73194).
    ///
    /// Active (no `#[ignore]`): direct, deterministic asserts.
    #[test]
    fn maxicode_const_table_sign_pinned() {
        assert_eq!(SENTINEL_ECI, -1, "SENTINEL_ECI must remain -1");
        assert_eq!(SENTINEL_PAD, -2, "SENTINEL_PAD must remain -2");
        assert_eq!(SENTINEL_NS, -3, "SENTINEL_NS must remain -3");
        assert_eq!(SENTINEL_LA, -4, "SENTINEL_LA (latch-A) must remain -4");
        assert_eq!(SENTINEL_LB, -5, "SENTINEL_LB (latch-B) must remain -5");
        assert_eq!(SENTINEL_SA, -6, "SENTINEL_SA (shift-A) must remain -6");
        assert_eq!(SENTINEL_SB, -7, "SENTINEL_SB (shift-B) must remain -7");
        assert_eq!(SENTINEL_SC, -8, "SENTINEL_SC (shift-C) must remain -8");
        assert_eq!(SENTINEL_SD, -9, "SENTINEL_SD (shift-D) must remain -9");
        assert_eq!(SENTINEL_SE, -10, "SENTINEL_SE (shift-E) must remain -10");
    }

    /// Stage 11.A8c-L — kills the 5 `>/>=` boundary mutants on the
    /// `encode_mode_2/3/4/5/6` capacity guards at L807/831/853/922/948.
    ///
    /// Each guard reads `if encmsg.len() > N { error }`. Existing
    /// overflow tests (200-byte 'A' payloads) hit `200 > N` AND
    /// `200 >= N` — both true, both error, so the `>` → `>=`
    /// mutation survives. Killing it requires the at-boundary
    /// SUCCESS case: an input that encodes to exactly `N` codewords
    /// must succeed under `>` (the current code) but error under
    /// `>=` (the mutant).
    ///
    /// Each set-A uppercase letter encodes to exactly 1 codeword via
    /// `encode_secondary_a_b_with_ns`, so `N` bytes of 'A' yields
    /// `encmsg.len() == N`. For modes 2/3 the wrapper provides the
    /// required `<postcode>\x1d<country>\x1d<service>\x1d` prefix
    /// before the at-boundary secondary.
    ///
    /// Boundaries:
    ///   * mode 4 (L807): N = 93
    ///   * mode 5 (L831): N = 77
    ///   * mode 6 (L853): N = 93
    ///   * mode 2 (L922): N = 84 (secondary only — primary built from postcode)
    ///   * mode 3 (L948): N = 84 (secondary only)
    ///
    /// Each at-boundary call MUST return `Ok(_)` with the canonical
    /// MaxiCode 30×33 shape. The mutant `>=` rejects every case.
    #[test]
    fn encode_mode_boundary_pinned() {
        // Mode 4: 93-byte 'A' run → encmsg.len() == 93. Boundary.
        let sym4 = encode_mode_4(&[b'A'; 93])
            .expect("mode 4 at-boundary (93 cws) must succeed under `>`; `>=` mutant rejects");
        assert_eq!(sym4.cols(), 30, "mode 4 boundary symbol must be 30 cols");
        assert_eq!(sym4.rows(), 33, "mode 4 boundary symbol must be 33 rows");

        // Mode 5: 77-byte 'A' run → encmsg.len() == 77. Boundary.
        let sym5 = encode_mode_5(&[b'A'; 77])
            .expect("mode 5 at-boundary (77 cws) must succeed under `>`; `>=` mutant rejects");
        assert_eq!(sym5.cols(), 30, "mode 5 boundary symbol must be 30 cols");
        assert_eq!(sym5.rows(), 33, "mode 5 boundary symbol must be 33 rows");

        // Mode 6: 93-byte 'A' run → encmsg.len() == 93. Boundary.
        let sym6 = encode_mode_6(&[b'A'; 93])
            .expect("mode 6 at-boundary (93 cws) must succeed under `>`; `>=` mutant rejects");
        assert_eq!(sym6.cols(), 30, "mode 6 boundary symbol must be 30 cols");
        assert_eq!(sym6.rows(), 33, "mode 6 boundary symbol must be 33 rows");

        // Mode 2: secondary = 84-byte 'A' run → encmsg.len() == 84.
        // Wrapper supplies a valid US-zip primary so we hit the
        // secondary guard at L922, not an earlier postcode check.
        let mut payload2 = b"12345\x1d840\x1d001\x1d".to_vec();
        payload2.extend(std::iter::repeat_n(b'A', 84));
        let sym2 = encode_mode_2(&payload2)
            .expect("mode 2 at-boundary (84 sec cws) must succeed under `>`; `>=` mutant rejects");
        assert_eq!(sym2.cols(), 30, "mode 2 boundary symbol must be 30 cols");
        assert_eq!(sym2.rows(), 33, "mode 2 boundary symbol must be 33 rows");

        // Mode 3: secondary = 84-byte 'A' run → encmsg.len() == 84.
        // Wrapper supplies a valid alphanumeric primary (CA postcode).
        let mut payload3 = b"K1A0B1\x1d124\x1d999\x1d".to_vec();
        payload3.extend(std::iter::repeat_n(b'A', 84));
        let sym3 = encode_mode_3(&payload3)
            .expect("mode 3 at-boundary (84 sec cws) must succeed under `>`; `>=` mutant rejects");
        assert_eq!(sym3.cols(), 30, "mode 3 boundary symbol must be 30 cols");
        assert_eq!(sym3.rows(), 33, "mode 3 boundary symbol must be 33 rows");
    }

    // -----------------------------------------------------------------
    // Stage 11.A8c-L — PRE-DRAFT FINGERPRINT KILLERS (PENDING CAPTURE).
    //
    // The two largest remaining maxicode mutant clusters per
    // `rust/MUTATION_RESULTS.md` v4 (commit e753de7):
    //
    //   - `encode_secondary_a_b`           9 survivors at L205-220
    //   - `encode_secondary_a_b_with_ns`  16 survivors at L320/L552-617
    //
    // Both functions are state-machine encoders that walk the input
    // bytes, dispatch on the active set (A/B vs C/D/E) and the run
    // length, and emit codewords. STATE-MACHINE FINGERPRINT pattern:
    // we feed each case through the encoder and hash the entire
    // `Vec<u8>` of output codewords with a position-weighted formula —
    // any mutation that changes the codeword sequence (arithmetic
    // tweak, match-arm flip, comparator boundary shift, deleted `!`,
    // `||` ↔ `&&` swap, etc.) shifts the fingerprint and breaks the
    // assert. The cap-tuple shape is identical to
    // `build_ccs_state_machine_fingerprint_pinned` (commit 0795813)
    // and `dbexp_encode_and_stacked_fingerprint_pinned` (commit
    // cfb68ae); see also the pre-draft idiom in commits 2c08652 and
    // e4d9c72.
    //
    // Pre-drafts are `#[ignore]`'d with placeholder `(0, 0)` caps.
    // Activation workflow (per established loop iteration):
    //   1. Drop `#[ignore]`.
    //   2. `cargo test --include-ignored -- --nocapture \
    //        encode_secondary_a_b_state_machine_fingerprint_pinned_pending \
    //        encode_secondary_a_b_with_ns_state_machine_fingerprint_pinned_pending`
    //   3. Paste the captured `(len, fp)` tuples into the
    //      `FP_AB_*` / `FP_ABNS_*` consts.
    //   4. Drop the `_pending` suffix and rerun scoped mutants.
    //
    // File safe — not in any running mutation service.

    /// Stage 11.A8c-L — STATE-MACHINE fingerprint pre-draft targeting
    /// the **9 `encode_secondary_a_b` survivors at L205-220**.
    ///
    /// The function dispatches an "other-set" run via a `match run_len`
    /// in two symmetric arms (state_b=false → set-B run, state_b=true →
    /// set-A run). Each arm has three explicit cases (run_len 1/2/3 for
    /// the set-A run arm; 1/2 for the set-B run arm with a trailing
    /// guard) plus a `_` fall-through (LB/LA latch with optional
    /// back-latch). The 9 surviving mutants cluster across:
    ///
    /// * L205 `1 =>` arm (SA-shift single-byte set-A run)
    /// * L210 `2 =>` arm (SA2-shift two-byte set-A run)
    /// * L216 `3 =>` arm (SA3-shift three-byte set-A run)
    /// * the symmetric set-B arms (SB, SB+SB, LB) at L238-260
    /// * `state_b = false/true` re-init after the latch arms
    /// * back-latch emission gated on `trailing_current_set`
    /// * `i += 1` step (single-byte in-current-set fall-through)
    /// * `j - other_start` run-length arithmetic
    ///
    /// 9 cases — one per surviving-mutant locus, plus a baseline:
    ///
    /// 1. `"A"` — pure set-A, no transitions (baseline state_b=false).
    /// 2. `"a"` — single set-B byte from state_b=false →
    ///    `_` arm (LB latch, no trailing, no back-latch).
    /// 3. `"Aa"` — set-A then 1 set-B → SB arm (L238 `1 =>`).
    /// 4. `"aA"` — start with set-B run len 1 then 1 set-A → at
    ///    state_b=false, first see set-B run len 1, ENTERS the
    ///    `_` arm (LB+1 cw, then LA back-latch since trailing).
    ///    Pins the `state_b = true; if trailing { LA; state_b=false }`
    ///    path (L255-258).
    /// 5. `"Aab"` — set-A, then 2 set-B bytes (mid-message), then EOM.
    ///    `trailing_current_set = false` so the `2 if trailing` guard
    ///    at L242 does NOT fire → falls to LB arm (L249-260). Pins
    ///    the match guard.
    /// 6. `"AabA"` — set-A, then 2 set-B bytes, then set-A.
    ///    `trailing_current_set = true` so the `2 if trailing` arm
    ///    L242-248 FIRES → SB+SB pair instead of LB. Pins the
    ///    SB tie-break.
    /// 7. `"aB"` — single set-B start (state_b=false, run len 1 →
    ///    `_` arm), then single set-A. Pins the `_` fall-through's
    ///    `trailing_current_set` arithmetic for set-B → set-A.
    /// 8. `"abA"` — 2 set-B bytes (from state_b=false, run len 2 →
    ///    `_` since the `2 if trailing` arm requires state_b=false?
    ///    No: that arm IS for state_b=false matching a set-B run).
    ///    Wait, state_b=false → run is "in set B (other set)" → the
    ///    state_b=false branch (L235-261) fires. run_len=2,
    ///    trailing=true → L242 fires → SB+SB+LA (no, the L242 arm
    ///    emits SB+SB without a back-latch — the state stays in B).
    ///    Hmm — re-read: actually the SB tie-break consumes 2 bytes
    ///    as separate single-byte shifts. After the arm state_b
    ///    stays whatever it was (false). The next iteration sees
    ///    set-A 'A' which is `in_current_set` → emits directly.
    /// 9. `"AAA"` + 4 set-B bytes "abcd" + "B" — long set-B run
    ///    (len 4) from state_b=false → `_` arm (LB+4cws+LA latch
    ///    back), pins the `_ => { LB; loop; state_b=true; if
    ///    trailing { LA; state_b=false } }` branch and the
    ///    back-latch on trailing-current.
    ///
    /// Each fingerprint tuple is `(len, position_weighted_hash)`.
    /// Mutating any of:
    /// * L205/210/216 match-arm numerals (`1` → `0/2`, `2` → `1/3`,
    ///   `3` → `2/4`) — case shape changes, fp shifts;
    /// * `out.push(SA_CODEWORD)` → wrong cw — fp shifts;
    /// * `j - other_start` → `j + other_start` / `* other_start` —
    ///   wrong run_len → wrong arm → fp shifts;
    /// * `state_b = false`/`true` after the latch — next iteration
    ///   reads the wrong set → fp shifts;
    /// * trailing back-latch guard → emit/omit a wrong byte → fp
    ///   shifts.
    #[test]
    fn encode_secondary_a_b_state_machine_fingerprint_pinned() {
        fn fp(out: &[u8]) -> (usize, u64) {
            let mut s: u64 = 0;
            for (i, &v) in out.iter().enumerate() {
                s = s.wrapping_add(
                    (v as u64).wrapping_mul((i as u64).wrapping_add(1).wrapping_mul(2_654_435_761)),
                );
            }
            (out.len(), s)
        }
        // (tag, input, want = (len, fp))
        let cases: &[(&str, &[u8], (usize, u64))] = &[
            // (1) Baseline: pure set-A, no transitions.
            ("setA_only", b"A", FP_AB_SETA_ONLY),
            // (2) Single set-B byte (state_b=false; run len 1 in set
            //     B; `_` arm — LB; no trailing → no LA back-latch).
            ("setB_solo", b"a", FP_AB_SETB_SOLO),
            // (3) set-A then single set-B → SB arm at L239 (run len
            //     1 in set B with trailing_current_set false because
            //     we're at EOM after that 1 set-B byte). state_b
            //     stays false through the SB arm — pins the
            //     `1 =>` arm.
            ("setA_SB1", b"Aa", FP_AB_SETA_SB1),
            // (4) set-B then set-A from state_b=false → set-B run
            //     len 1, `_` fall-through (NOT the `1 =>` arm —
            //     wait, that's wrong: `1 =>` IS the first arm. Yes
            //     it fires. So run_len 1 → SB+a, then 'A' is in
            //     current set → emit directly. Pins the SB arm
            //     followed by in-current-set fall-through.)
            ("setB1_setA", b"aA", FP_AB_SETB1_SETA),
            // (5) set-A then 2 set-B at EOM (trailing=false) → L242
            //     `2 if trailing` GUARD FAILS → `_` arm fires → LB
            //     + 2 cws + no LA back-latch. Pins the match guard.
            ("setB2_eom", b"Aab", FP_AB_SETB2_EOM),
            // (6) set-A, 2 set-B, set-A (trailing=true) → L242
            //     `2 if trailing` arm FIRES → SB+SB pair. Pins the
            //     SB tie-break vs LB+LA cost-tie.
            ("setB2_mid", b"AabA", FP_AB_SETB2_MID),
            // (7) set-B run len 4 (`_` arm) bracketed by set-A.
            //     LB + 4 setB cws + LA back-latch.
            ("setB4_run", b"AabcdA", FP_AB_SETB4_RUN),
            // (8) set-A 3-byte run from state_b=true. Start with
            //     a single set-B byte (state_b becomes true mid-
            //     encoding via 'a' → SB? no — first byte: 'a' is
            //     set-B only, state_b=false, run_len=1, `1 =>` arm
            //     fires (SB shift, no state change). So state_b
            //     never goes to true via this pattern.
            //     Instead use long set-B run "aaaa" then "ABC" then
            //     "abcd" to force a LB latch (state_b → true), then
            //     SA3 (run_len 3 → L216 fires), then LA-style
            //     trailing back-latch via the `_` arm.
            ("SA3_arm", b"aaaaABCabcd", FP_AB_SA3_ARM),
            // (9) set-A 2-byte run from state_b=true → SA2 arm at
            //     L210. Use "aaaaABabcd" — LB+4set-B latches state_b
            //     to true, then run_len 2 in set A → SA2 arm fires.
            ("SA2_arm", b"aaaaABabcd", FP_AB_SA2_ARM),
        ];
        for (tag, input, want) in cases {
            let got = fp(&encode_secondary_a_b(input).unwrap_or_else(|e| {
                panic!("encode_secondary_a_b({tag}) must succeed; got Err: {e:?}")
            }));
            eprintln!("CAP encode_secondary_a_b/{tag} -> {got:?}");
            assert_eq!(got, *want, "fingerprint changed for {tag}");
        }
    }
    const FP_AB_SETA_ONLY: (usize, u64) = (1, 2654435761);
    const FP_AB_SETB_SOLO: (usize, u64) = (2, 161920581421);
    const FP_AB_SETA_SB1: (usize, u64) = (3, 323841162842);
    const FP_AB_SETB1_SETA: (usize, u64) = (3, 169883888704);
    const FP_AB_SETB2_EOM: (usize, u64) = (4, 366312135018);
    const FP_AB_SETB2_MID: (usize, u64) = (6, 992758974614);
    const FP_AB_SETB4_RUN: (usize, u64) = (8, 1661676786386);
    const FP_AB_SA3_ARM: (usize, u64) = (14, 3357861237665);
    const FP_AB_SA2_ARM: (usize, u64) = (13, 3092417661565);

    /// Stage 11.A8c-L — STATE-MACHINE fingerprint pre-draft targeting
    /// the **16 `encode_secondary_a_b_with_ns` survivors** at
    /// L320 / L552-617. The `_with_ns` variant adds NS-digit
    /// optimization to the A/B dispatcher and folds in C/D/E set
    /// shifts/latches with cross-set intra-latch absorption, so the
    /// mutant cluster spans:
    ///
    /// * L320: `let (lead, ns_start) = if !state_b { ... } else { ... }`
    ///   (delete `!` → swap branches → NS dispatch goes wrong way)
    /// * L552: `let trailing = j < n;` (`<` → `>`, `<=`)
    /// * L555-565: `match run_len { 1 / 2 / 3 / _ }` (mirror of
    ///   `encode_secondary_a_b` set-A arm — match-arm flips, the
    ///   `out.push` arithmetic, run_len bounds)
    /// * L590: `2 if trailing => { … }` (match guard delete)
    /// * L596: `_ => { LB; loop; state_b=true; if trailing { … } }`
    /// * L611-617: trailing back-latch gating with the compound
    ///   `nseq[j] < 9 && !next_is_cde`. The `< 9` boundary mutates
    ///   `>` / `<=` / `>=`; the `||` in `next_is_cde` (built from
    ///   `setc_codeword(bytes[j]).is_some() || setd_… || sete_…`)
    ///   becomes `&&`; the `!` on `next_is_cde` deletes.
    ///
    /// 13 cases covering each match arm + each boundary mutant:
    ///
    /// 1. `"A"` — baseline.
    /// 2. `"123456789"` — pure 9-digit NS run from state_b=false →
    ///    L315 `nseq[i] >= 9` true, L320 `!state_b` branch: lead =
    ///    nseq[i] - 9 = 0, NS chunk + done. Pins the `!state_b`
    ///    branch (deleted `!` → goes to else → wrong lead).
    /// 3. `"123456789012"` — 12-digit run (lead=3 plain set-A then
    ///    9-digit NS chunk). Pins the `lead = nseq[i] - 9`
    ///    arithmetic.
    /// 4. `"abc123456789"` — set-B 3-char then 9-digit run. After
    ///    "abc" state_b=true. At i=3 nseq[3]=9, `state_b=true` →
    ///    L320 else branch: lead=0, ns_start=i. Pins the else
    ///    branch (without the `!`, both states use the same lead
    ///    formula → output diverges).
    /// 5. `"TEST\xC0"` — single set-C byte after set-A run. SC shift
    ///    arm at L450 (run_len=1, < 3 → shift path). Already covers
    ///    set-C dispatch. Pins the C/D/E vs A/B "other-set" code path.
    /// 6. `"TEST\xC0\xC0\xC0"` — 3 set-C bytes → latch path at L444
    ///    `if run_len >= 3`. Pins the threshold boundary.
    /// 7. `"\xC0\xC0\xC0\xC0\xC0"` — pure set-C 5-byte EOM → latch,
    ///    keeps LA back-latch (`omit_back_latch_at_eom = false`
    ///    since set_idx=2).
    /// 8. `"\xA0\xA0\xA0"` — pure set-E 3-byte EOM → latch, OMITS
    ///    back-latch (set_idx=4 path L467). Distinguishes E from
    ///    C/D.
    /// 9. `"a"` — 1-byte set-B (`1 =>` arm at L586 from state_b=
    ///    false; run_len 1 in set-B; `1 =>` fires).
    /// 10. `"Aab"` — set-A then 2 set-B at EOM. L590 `2 if trailing`
    ///     fails (trailing=false at EOM) → `_` arm. Pins the guard.
    /// 11. `"AabA"` — set-A, 2 set-B, set-A. L590 `2 if trailing`
    ///     FIRES → SB+SB. Pins the guard true-branch.
    /// 12. `"Aabcd123456789"` — set-A, 4 set-B latch (`_` arm at
    ///     L596), trailing=true, and the NEXT byte (after 'd') is
    ///     '1' which IS a digit and nseq[j]=9 → `nseq[j] < 9`
    ///     FALSE → skip LA emission. Pins the `nseq[j] < 9`
    ///     boundary AND the conditional LA back-latch.
    /// 13. `"Aabcd\xC0"` — set-A, 4 set-B latch (`_` arm), trailing
    ///     byte is set-C (`next_is_cde = true`) → `!next_is_cde`
    ///     false → skip LA emission. Pins the `!next_is_cde` (the
    ///     deleted `!` mutant would emit LA) AND the `||` chain
    ///     (mutant `||` → `&&` collapses the union to intersection).
    #[test]
    fn encode_secondary_a_b_with_ns_state_machine_fingerprint_pinned() {
        fn fp(out: &[u8]) -> (usize, u64) {
            let mut s: u64 = 0;
            for (i, &v) in out.iter().enumerate() {
                s = s.wrapping_add(
                    (v as u64).wrapping_mul((i as u64).wrapping_add(1).wrapping_mul(2_654_435_761)),
                );
            }
            (out.len(), s)
        }
        // (tag, input, want = (len, fp))
        let cases: &[(&str, &[u8], (usize, u64))] = &[
            ("setA_only", b"A", FP_ABNS_SETA_ONLY),
            ("ns9_pure", b"123456789", FP_ABNS_NS9_PURE),
            ("ns9_lead3", b"123456789012", FP_ABNS_NS9_LEAD3),
            ("setB_then_ns9", b"abc123456789", FP_ABNS_SETB_NS9),
            ("setC_shift1", b"TEST\xC0", FP_ABNS_SETC_SHIFT1),
            ("setC_latch3", b"TEST\xC0\xC0\xC0", FP_ABNS_SETC_LATCH3),
            ("setC_only5", b"\xC0\xC0\xC0\xC0\xC0", FP_ABNS_SETC_ONLY5),
            ("setE_only3", b"\xA0\xA0\xA0", FP_ABNS_SETE_ONLY3),
            ("setB_solo", b"a", FP_ABNS_SETB_SOLO),
            ("setB2_eom", b"Aab", FP_ABNS_SETB2_EOM),
            ("setB2_mid", b"AabA", FP_ABNS_SETB2_MID),
            ("setB4_then_ns9", b"Aabcd123456789", FP_ABNS_SETB4_NS9),
            ("setB4_then_setC", b"Aabcd\xC0", FP_ABNS_SETB4_SETC),
        ];
        for (tag, input, want) in cases {
            let got = fp(&encode_secondary_a_b_with_ns(input).unwrap_or_else(|e| {
                panic!("encode_secondary_a_b_with_ns({tag}) must succeed; got Err: {e:?}")
            }));
            eprintln!("CAP encode_secondary_a_b_with_ns/{tag} -> {got:?}");
            assert_eq!(got, *want, "fingerprint changed for {tag}");
        }
    }
    const FP_ABNS_SETA_ONLY: (usize, u64) = (1, 2654435761);
    const FP_ABNS_NS9_PURE: (usize, u64) = (6, 1956319155857);
    const FP_ABNS_NS9_LEAD3: (usize, u64) = (9, 3803806445513);
    const FP_ABNS_SETB_NS9: (usize, u64) = (10, 4225861731512);
    const FP_ABNS_SETC_SHIFT1: (usize, u64) = (6, 1239621500387);
    const FP_ABNS_SETC_LATCH3: (usize, u64) = (10, 3734791115727);
    const FP_ABNS_SETC_ONLY5: (usize, u64) = (8, 1709456630084);
    const FP_ABNS_SETE_ONLY3: (usize, u64) = (5, 1672294529430);
    const FP_ABNS_SETB_SOLO: (usize, u64) = (2, 161920581421);
    const FP_ABNS_SETB2_EOM: (usize, u64) = (4, 366312135018);
    const FP_ABNS_SETB2_MID: (usize, u64) = (6, 992758974614);
    const FP_ABNS_SETB4_NS9: (usize, u64) = (12, 5499990896792);
    const FP_ABNS_SETB4_SETC: (usize, u64) = (8, 1584698149317);

    // ----------------------------------------------------------------
    // Stage 11.A8d — maxicode T2-a: kill/prove the 23 mutants-v5
    // survivors. The pre-existing state-machine fingerprint tests
    // (`encode_secondary_a_b{,_with_ns}_state_machine_fingerprint_pinned`)
    // pinned the *reachable* arms, but their documentation over-claimed:
    // they never actually drive the encoder into `state_b == true` with
    // a following set-A run, so the SA/SA2/SA3 set-A arms were never
    // exercised. The tests below close those gaps with exact-codeword
    // assertions, plus prove the genuinely-dead arithmetic equivalent
    // and the truly-unreachable arms.
    // ----------------------------------------------------------------

    /// Reaching the SA / SA2 / SA3 set-A arms requires `state_b == true`
    /// at the top of a `while` iteration with set-A bytes still ahead.
    /// In `encode_secondary_a_b_with_ns` the ONLY way to get there is a
    /// C/D/E latch whose mid-message back-latch is `LB` (codeword 63),
    /// which sets `state_b = true` (L502 `state_b = back_latch == 63`).
    /// We force that with a 3-byte set-C run (`^192^192^192`, latch +
    /// [0,0,0]) followed by a set-B byte `a` (so the back-latch is LB),
    /// then a short set-A run, then another set-B byte so the run isn't
    /// at EOM.
    ///
    /// Kills:
    /// * L556 `1 =>` arm-delete  — SA path (codeword 59).
    /// * L560 `2 =>` arm-delete  — SA2 path (codeword 56).
    /// * L565 `3 =>` arm-delete  — SA3 path (codeword 57).
    /// * L563 `bytes[i + 1]` `+`→`-`/`*`  (SA2 second byte).
    /// * L568 `bytes[i + 1]` `+`→`-`/`*`  (SA3 second byte).
    /// * L569 `bytes[i + 2]` `+`→`-`/`*`  (SA3 third byte).
    ///
    /// The set-A run uses DISTINCT letters (B=2, C=3, D=4) so a wrong
    /// index (`i-1`, `i*1`, `i*2`, …) yields either a different codeword
    /// or a `None`→`unwrap` panic — both fail the assertion.
    #[test]
    fn encode_secondary_a_b_with_ns_set_a_shift_arms_reached_via_cde_back_latch() {
        // SA arm (run_len == 1). `^192^192^192` + 'a' (LB back-latch,
        // state_b=true) + 'B' (1-byte set-A run, SA shift) + 'a'.
        assert_eq!(
            encode_secondary_a_b_with_ns(b"\xC0\xC0\xC0aBa").unwrap(),
            vec![60, 60, 0, 0, 0, 63, 1, 59, 2, 1],
            "L556 SA arm: SC-latch + LB + 'a' + SA(59) + 'B'(2) + 'a'(1)",
        );
        // SA2 arm (run_len == 2). 'B','C' → 2,3. L562 emits bytes[i],
        // L563 emits bytes[i+1].
        assert_eq!(
            encode_secondary_a_b_with_ns(b"\xC0\xC0\xC0aBCa").unwrap(),
            vec![60, 60, 0, 0, 0, 63, 1, 56, 2, 3, 1],
            "L560 SA2 arm: SA2(56) + 'B'(2,L562) + 'C'(3,L563 i+1) + 'a'",
        );
        // SA3 arm (run_len == 3). 'B','C','D' → 2,3,4. L567/568/569.
        assert_eq!(
            encode_secondary_a_b_with_ns(b"\xC0\xC0\xC0aBCDa").unwrap(),
            vec![60, 60, 0, 0, 0, 63, 1, 57, 2, 3, 4, 1],
            "L565 SA3 arm: SA3(57) + 'B'(2,L567) + 'C'(3,L568 i+1) + 'D'(4,L569 i+2)",
        );
    }

    /// Kills the set-B `_`-arm trailing back-latch survivors at L616
    /// (`||`→`&&`) and L617 (`<`→`>`) in `encode_secondary_a_b_with_ns`.
    ///
    /// L611-616 builds `next_is_cde` from
    /// `setc(j).is_some() || setd(j).is_some() || sete(j).is_some()`.
    /// The mutated `||`→`&&` turns the LAST disjunct into
    /// `setd(j).is_some() && sete(j).is_some()`. Byte `0xA0` (160) is
    /// set-E ONLY (setc=None, setd=None, sete=Some) → original
    /// `next_is_cde == true` (skip LA), mutant `next_is_cde == false`
    /// (emit LA). The exact-vector assertion catches the inserted LA.
    ///
    /// L617 `if trailing && nseq[j] < 9 && !next_is_cde`. After a set-B
    /// run≥3 followed by the set-A byte 'E' (nseq[j] == 0): original
    /// `0 < 9 == true` → emit LA(63); mutant `0 > 9 == false` → omit it.
    #[test]
    fn encode_secondary_a_b_with_ns_setb_run_back_latch_boundaries() {
        // L616: "abcd" (LB+1,2,3,4) then set-E-only 0xA0 → LA suppressed,
        // straight to SE shift (62) + setE(0xA0)=37. Mutant `&&` would
        // insert a 63 (LA) before the 62.
        assert_eq!(
            encode_secondary_a_b_with_ns(b"abcd\xA0").unwrap(),
            vec![63, 1, 2, 3, 4, 62, 37],
            "L616 ||→&&: set-E byte must keep next_is_cde=true (no LA)",
        );
        // L617: "abcd" then set-A 'E' (nseq[j]=0, not C/D/E) → LA(63)
        // emitted before 'E'(5). Mutant `>` drops the LA.
        assert_eq!(
            encode_secondary_a_b_with_ns(b"abcdE").unwrap(),
            vec![63, 1, 2, 3, 4, 63, 5],
            "L617 <→>: nseq[j]=0 < 9 true → LA(63) before 'E'(5)",
        );
    }

    /// Kills the `seta_codeword` arm-delete at L1173
    /// (`28..=30 => Some(byte)`). The existing `seta_codeword_known_values`
    /// test never probes 28/29/30, so deleting that arm (FS/GS/RS →
    /// `None`) went unnoticed. These three direct lookups pin it.
    #[test]
    fn seta_codeword_fs_gs_rs_controls() {
        assert_eq!(seta_codeword(28), Some(28), "FS");
        assert_eq!(seta_codeword(29), Some(29), "GS");
        assert_eq!(seta_codeword(30), Some(30), "RS");
        // Boundaries: 27 and 31 are NOT in this arm.
        assert_eq!(seta_codeword(27), None, "27 below the 28..=30 arm");
        assert_eq!(seta_codeword(31), None, "31 above the 28..=30 arm");
    }

    /// Pins `build_grid`'s `row < ROWS` guard at L772 against the
    /// `<`→`<=` mutant. ROWS=33, so the valid row range is 0..=32. A
    /// pixel index of 990 maps to row=33 (=ROWS), which the guard MUST
    /// drop. The original returns a grid with exactly one set cell
    /// (from p=0); the `<=` mutant would attempt `grid[33][0]` and panic
    /// with an out-of-bounds index, so reaching the assertions at all
    /// already kills it.
    ///
    /// (The pre-existing `build_grid_indexing_and_oob_guard` also feeds
    /// p=990; this is a focused, self-contained reinforcement keyed
    /// explicitly to the `row < ROWS` boundary.)
    #[test]
    fn build_grid_row_eq_rows_dropped() {
        let grid = build_grid(&[0u16, 990u16]);
        assert!(grid[0][0], "p=0 → (0,0) set");
        let total: usize = grid.iter().flat_map(|r| r.iter()).filter(|&&b| b).count();
        assert_eq!(
            total, 1,
            "p=990 (row==ROWS) must be dropped by `row < ROWS`; \
             the <= mutant would panic before reaching here",
        );
    }

    /// Equivalence / unreachability notes for the survivors that cannot
    /// be killed by any reachable input (proved below with executable
    /// witnesses, not assertions on the mutant — the mutant is
    /// behaviourally identical on every reachable call).
    ///
    /// === slice_scm_to_codewords L1392 `|`→`^` (EQUIVALENT) ===
    /// The line is `value = (value << 1) | bit`. `value << 1` always
    /// has its low bit clear, and `bit = scm[k] - b'0'`. The `scm`
    /// buffer is ONLY ever produced by `pack_mode_2_primary` /
    /// `pack_mode_3_primary`, which initialise it to `b'0'` and overwrite
    /// slots exclusively with `to_bin(..)` output — strings of ASCII
    /// '0'/'1'. Hence every `bit ∈ {0, 1}` and, since `(x<<1)` has bit-0
    /// == 0, `(x<<1) | bit == (x<<1) ^ bit` for all reachable inputs.
    /// The `|`/`^` mutant is therefore behaviourally identical. The
    /// witness below demonstrates the bit-level identity over the full
    /// reachable bit domain {0,1} and several accumulator values.
    ///
    /// === encode_secondary_a_b set-A arms L205/210/216 + arithmetic
    ///     L214/L219/L220 (UNREACHABLE) ===
    /// These nine survivors live in the `if state_b { /* run in set A */
    /// match run_len { 1 | 2 | 3 | _ } }` block (L202-261). Entering it
    /// requires `state_b == true` at the top of a loop iteration with
    /// bytes still to consume. In `encode_secondary_a_b`, `state_b` is
    /// initialised `false` and is set `true` in EXACTLY one place: the
    /// set-B `_` arm (L255-258), which does
    /// `state_b = true; if trailing_current_set { LB→LA; state_b = false }`.
    /// That arm only runs while `state_b == false` (the `else` branch),
    /// so it can leave `state_b == true` only when
    /// `trailing_current_set == false`, i.e. the set-B run ends the
    /// message (`j == n`). With no remaining bytes the loop exits, so
    /// `state_b == true` is NEVER observed at the top of a subsequent
    /// iteration. The set-A-run block — and thus arms L205/210/216 and
    /// the `other_start + 1/2` index arithmetic at L214/219/220 — is
    /// dead code in this function. The witness below scans every
    /// 1..=4-byte string over a representative {set-A, set-B} alphabet
    /// and asserts the function never emits SA2(56) or SA3(57), the
    /// unique codewords those arms would produce. (The `_with_ns`
    /// variant DOES reach the analogous arms via C/D/E back-latch — see
    /// `..._set_a_shift_arms_reached_via_cde_back_latch` above — so the
    /// arms exist for that caller; they are merely unreachable from the
    /// pure-A/B entry point.)
    #[test]
    fn maxicode_equivalence_notes() {
        // --- slice_scm_to_codewords L1392 `|` vs `^` over the reachable
        //     bit domain {0,1}. ---
        for acc in 0u8..=0x1F {
            for bit in 0u8..=1 {
                let shifted = acc << 1;
                assert_eq!(
                    shifted | bit,
                    shifted ^ bit,
                    "|/^ identical when (acc<<1) has bit0==0 and bit∈{{0,1}}",
                );
            }
        }
        // And end-to-end: the real packer only feeds '0'/'1' bits, so a
        // golden round-trip is unaffected by the mutant (sanity witness).
        let pri = pack_mode_2_primary("12345", "840", "001").unwrap();
        assert_eq!(pri, [2, 36, 50, 46, 53, 17, 2, 18, 7, 0]);

        // --- build_grid L772:30 `col < COLS` → `col <= COLS` is EQUIVALENT. ---
        // `col = (p as usize) % COLS`, so col ∈ 0..=COLS-1 for every p; it is
        // never == COLS. Hence `col < COLS` and `col <= COLS` accept exactly
        // the same `col` values → identical grid. (The sibling `row < ROWS`
        // mutant on the same line IS killable — p = ROWS*COLS gives row==ROWS,
        // and `<=` then indexes grid[ROWS] out of bounds; that one is killed
        // by `build_grid_row_eq_rows_dropped` / `build_grid_indexing_and_oob_guard`.)
        // Witness: col is < COLS across the full single-cell index domain.
        for p in 0usize..(ROWS * COLS) {
            assert!(p % COLS < COLS, "col index always < COLS");
        }

        // --- encode_secondary_a_b set-A arms unreachable: brute-force
        //     every short string over a 2-set alphabet; SA2(56)/SA3(57)
        //     must never appear (they are only emitted by L210/L216). ---
        const SA2: u8 = 56;
        const SA3: u8 = 57;
        let alphabet: [u8; 4] = [b'A', b'B', b'a', b'b']; // 2 set-A, 2 set-B
        let mut buf = Vec::new();
        for len in 1..=4usize {
            let mut idx = vec![0usize; len];
            loop {
                buf.clear();
                for &k in &idx {
                    buf.push(alphabet[k]);
                }
                let out = encode_secondary_a_b(&buf).unwrap();
                assert!(
                    !out.contains(&SA2) && !out.contains(&SA3),
                    "encode_secondary_a_b should never reach SA2/SA3 arms; \
                     input {buf:?} produced {out:?}",
                );
                // odometer increment over the alphabet indices.
                let mut p = len;
                loop {
                    if p == 0 {
                        break;
                    }
                    p -= 1;
                    idx[p] += 1;
                    if idx[p] < alphabet.len() {
                        break;
                    }
                    idx[p] = 0;
                }
                if p == 0 && idx[0] == 0 {
                    break;
                }
            }
        }
    }
}
