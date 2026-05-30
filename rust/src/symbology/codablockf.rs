//! Codablock-F (stacked Code 128).
//!
//! Codablock-F is a 2D stacked symbology that lays out one or more Code 128
//! rows on top of each other. Each row starts with a Code 128 Start-A
//! pattern, a subset selector, and a row indicator that lets a scanner
//! re-assemble the rows out of order. The last row carries two extra
//! check characters (k1, k2) computed over the entire data stream.
//!
//! [`encode`] is byte-exact against bwip-js's codablockf encoder for the
//! known inputs in this module's tests — codeword sequence matches
//! `debugcws` output and the rendered bit pattern matches the bwip-js
//! SVG `<rect>` geometry.
//!
//! Reference: bwip-js src/bwipp.js `bwipp_codablockf` (2026-03-31 vendor)
//! and BWIPP `codablockf.ps`.

use crate::encoding::{LinearPattern, StackedPattern};
use crate::error::Error;
use crate::options::Options;

// Special markers used during encoding. Negative i32 values can never
// collide with the 0..127 ASCII byte range.
const SWA: i32 = -1;
const SWB: i32 = -2;
const SWC: i32 = -3;
const SFT: i32 = -4;
const FN1: i32 = -5;
const FN2: i32 = -6;
const FN3: i32 = -7;
const FN4: i32 = -8;
const STA: i32 = -9;
const STP: i32 = -10;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Subset {
    A,
    B,
    C,
}

fn seta(tok: i32) -> Option<u32> {
    match tok {
        0..=31 => Some((tok + 64) as u32),
        32..=95 => Some((tok - 32) as u32),
        FN3 => Some(96),
        FN2 => Some(97),
        SFT => Some(98),
        SWC => Some(99),
        SWB => Some(100),
        FN4 => Some(101),
        FN1 => Some(102),
        STA => Some(103),
        STP => Some(104),
        _ => None,
    }
}

fn setb(tok: i32) -> Option<u32> {
    match tok {
        32..=127 => Some((tok - 32) as u32),
        FN3 => Some(96),
        FN2 => Some(97),
        SFT => Some(98),
        SWC => Some(99),
        FN4 => Some(100),
        SWA => Some(101),
        FN1 => Some(102),
        STA => Some(103),
        STP => Some(104),
        _ => None,
    }
}

fn in_set_a(tok: i32) -> bool {
    seta(tok).is_some()
}

fn in_set_b(tok: i32) -> bool {
    setb(tok).is_some()
}

/// Subset C accepts digits (0..9), FN1, and the shared control markers.
fn in_set_c(tok: i32) -> bool {
    if (b'0' as i32..=b'9' as i32).contains(&tok) {
        return true;
    }
    matches!(tok, FN1 | SWB | SWA | STA | STP)
}

fn anotb(tok: i32) -> bool {
    in_set_a(tok) && !in_set_b(tok)
}

fn bnota(tok: i32) -> bool {
    in_set_b(tok) && !in_set_a(tok)
}

/// `numsscr` from BWIPP: starting at position `p`, count how many
/// "digit slots" (s) and how many "digit characters" (n) are available
/// before something that can't be encoded in subset C interrupts the run.
/// FN1 inside a digit run counts as half a digit-pair slot.
fn numsscr(msg: &[i32], p: usize) -> (i32, i32) {
    let mut n: i32 = 0;
    let mut s: i32 = 0;
    let mut p = p;
    while p < msg.len() {
        if p != 0 && msg[p - 1] == FN4 {
            break;
        }
        let c = msg[p];
        if !in_set_c(c) {
            break;
        }
        if c == FN1 {
            if s % 2 == 0 {
                s += 1;
            } else {
                break;
            }
        }
        n += 1;
        s += 1;
        p += 1;
    }
    (n, s)
}

fn push_seta(cws: &mut Vec<u32>, v: i32) {
    cws.push(seta(v).expect("internal: enca on non-set-A value"));
}

fn push_setb(cws: &mut Vec<u32>, v: i32) {
    cws.push(setb(v).expect("internal: encb on non-set-B value"));
}

fn push_c_fn1(cws: &mut Vec<u32>) {
    cws.push(102);
}

fn push_c_pair(cws: &mut Vec<u32>, a: i32, b: i32) {
    debug_assert!((b'0' as i32..=b'9' as i32).contains(&a));
    debug_assert!((b'0' as i32..=b'9' as i32).contains(&b));
    cws.push(((a - 48) * 10 + (b - 48)) as u32);
}

/// Push `n` alternating switch codewords to finish a row, starting from
/// the current `cset` and updating it.
fn pad_row(cws: &mut Vec<u32>, cset: &mut Subset, n: usize) {
    for _ in 0..n {
        match *cset {
            Subset::A => {
                push_seta(cws, SWC);
                *cset = Subset::C;
            }
            Subset::B => {
                push_setb(cws, SWC);
                *cset = Subset::C;
            }
            Subset::C => {
                cws.push(100); // swb in set C
                *cset = Subset::B;
            }
        }
    }
}

/// BWIPP `encafitsrow`: encode the current character into subset A,
/// handling the special case of FN4 + a following byte that share the
/// remaining two-cell tail of a row. Returns `true` if the byte was
/// encoded; `false` if the FN4-pair didn't fit (caller falls through).
fn enc_a_fits_row(cws: &mut Vec<u32>, i: &mut usize, msg: &[i32], rem: i32) -> bool {
    if rem <= 2 && msg[*i] == FN4 {
        let fits = rem == 2 && msg[*i + 1] <= 95;
        if fits {
            push_seta(cws, FN4);
            push_seta(cws, msg[*i + 1]);
            *i += 2;
        }
        fits
    } else {
        push_seta(cws, msg[*i]);
        *i += 1;
        true
    }
}

fn enc_b_fits_row(cws: &mut Vec<u32>, i: &mut usize, msg: &[i32], rem: i32) -> bool {
    if rem <= 2 && msg[*i] == FN4 {
        let fits = rem == 2 && msg[*i + 1] >= 32;
        if fits {
            push_setb(cws, FN4);
            push_setb(cws, msg[*i + 1]);
            *i += 2;
        }
        fits
    } else {
        push_setb(cws, msg[*i]);
        *i += 1;
        true
    }
}

/// Generate the Codablock-F codeword sequence for `msg` with `columns`
/// data columns per row.
///
/// `msg` is the input as i32s: ASCII bytes (0..=127) for normal characters
/// or one of [`FN1`], [`FN2`], [`FN3`], [`FN4`] for Code 128 function
/// markers. Returns codewords in row-major order; each row is
/// `columns + 5` codewords wide (start, subset selector, row indicator,
/// `columns` data slots, row check, stop). The very last row has the
/// final two data slots replaced by the k1/k2 global check characters.
pub(crate) fn codewords(msg: &[i32], columns: usize) -> Result<Vec<u32>, &'static str> {
    if !(4..=62).contains(&columns) {
        return Err("Codablock-F must have 4 to 62 columns");
    }
    let c = columns;
    let row_width = c + 5;

    let len = msg.len();
    let mut next_anotb = vec![9999u32; len + 1];
    let mut next_bnota = vec![9999u32; len + 1];
    for i in (0..len).rev() {
        next_anotb[i] = if anotb(msg[i]) {
            0
        } else {
            next_anotb[i + 1].saturating_add(1)
        };
        next_bnota[i] = if bnota(msg[i]) {
            0
        } else {
            next_bnota[i + 1].saturating_add(1)
        };
    }
    let abeforeb = |i: usize| -> bool { next_anotb[i] < next_bnota[i] };
    let bbeforea = |i: usize| -> bool { next_bnota[i] < next_anotb[i] };

    let mut cws: Vec<u32> = Vec::with_capacity(row_width * 4);
    let mut i: usize = 0;
    let mut r: u32 = 1;
    let mut last_row = false;
    let mut cset = Subset::B;

    while !last_row {
        if r > 44 {
            return Err("Codablock-F: maximum length exceeded");
        }

        // [0]: Start A
        push_seta(&mut cws, STA);

        let (_, nums) = if i < msg.len() {
            numsscr(msg, i)
        } else {
            (-1, -1)
        };

        // [1]: subset selector for this row's data.
        if msg.is_empty() {
            push_seta(&mut cws, SWB);
            cset = Subset::B;
        } else if nums >= 2 {
            push_seta(&mut cws, SWC);
            cset = Subset::C;
        } else if i < msg.len() && abeforeb(i) {
            push_seta(&mut cws, SFT);
            cset = Subset::A;
        } else {
            push_seta(&mut cws, SWB);
            cset = Subset::B;
        }

        // [2]: row-indicator placeholder, filled in after all rows are known.
        cws.push(0);

        // Inner row-data loop.
        let mut end_of_row = false;
        while !end_of_row && i < msg.len() {
            let j = cws.len();
            let rem = (c as i32 + 3) - (j % row_width) as i32;
            let (_, nums2) = numsscr(msg, i);
            let remnums = nums2.min(rem * 2);

            // Branch selection: at most one branch fires per iteration.
            // We mirror BWIPP exactly so each `continue` walks the same
            // path it would in the postscript original.
            let mut acted = false;

            // Branch 1: in A/B looking at a digit run worth shifting to C.
            if matches!(cset, Subset::A | Subset::B) && remnums >= 4 && msg[i] != FN1 {
                if remnums % 2 == 0 && rem >= 3 {
                    match cset {
                        Subset::A => push_seta(&mut cws, SWC),
                        Subset::B => push_setb(&mut cws, SWC),
                        Subset::C => unreachable!(),
                    }
                    cset = Subset::C;
                    for _ in 0..2 {
                        if msg[i] == FN1 {
                            push_c_fn1(&mut cws);
                            i += 1;
                        } else {
                            push_c_pair(&mut cws, msg[i], msg[i + 1]);
                            i += 2;
                        }
                    }
                    acted = true;
                } else if remnums % 2 != 0 && rem >= 4 {
                    let m = msg[i];
                    match cset {
                        Subset::A => push_seta(&mut cws, m),
                        Subset::B => push_setb(&mut cws, m),
                        Subset::C => unreachable!(),
                    }
                    i += 1;
                    match cset {
                        Subset::A => push_seta(&mut cws, SWC),
                        Subset::B => push_setb(&mut cws, SWC),
                        Subset::C => unreachable!(),
                    }
                    cset = Subset::C;
                    for _ in 0..2 {
                        if msg[i] == FN1 {
                            push_c_fn1(&mut cws);
                            i += 1;
                        } else {
                            push_c_pair(&mut cws, msg[i], msg[i + 1]);
                            i += 2;
                        }
                    }
                    acted = true;
                }
            }

            if !acted {
                // Branch 2: in B but next char is A-only.
                if cset == Subset::B && anotb(msg[i]) && rem >= 2 {
                    if i + 1 < msg.len() && bbeforea(i + 1) {
                        push_setb(&mut cws, SFT);
                        push_seta(&mut cws, msg[i]);
                        i += 1;
                    } else {
                        push_setb(&mut cws, SWA);
                        cset = Subset::A;
                        push_seta(&mut cws, msg[i]);
                        i += 1;
                    }
                    acted = true;
                }
            }

            if !acted {
                // Branch 3: in A but next char is B-only.
                if cset == Subset::A && bnota(msg[i]) && rem >= 2 {
                    if i + 1 < msg.len() && abeforeb(i + 1) {
                        push_seta(&mut cws, SFT);
                        push_setb(&mut cws, msg[i]);
                        i += 1;
                    } else {
                        push_seta(&mut cws, SWB);
                        cset = Subset::B;
                        push_setb(&mut cws, msg[i]);
                        i += 1;
                    }
                    acted = true;
                }
            }

            if !acted {
                // Branch 4: in C but not enough digits to stay; switch out.
                if cset == Subset::C && remnums < 2 && rem >= 2 {
                    if abeforeb(i) {
                        cws.push(101); // setC[SWA] = 101
                        cset = Subset::A;
                        if enc_a_fits_row(&mut cws, &mut i, msg, rem - 1) {
                            acted = true;
                        }
                    } else {
                        cws.push(100); // setC[SWB] = 100
                        cset = Subset::B;
                        if enc_b_fits_row(&mut cws, &mut i, msg, rem - 1) {
                            acted = true;
                        }
                    }
                    // If !acted, BWIPP falls through to branches 5/6
                    // with cset now updated.
                }
            }

            if !acted {
                // Branch 5: in A and char fits.
                if cset == Subset::A
                    && in_set_a(msg[i])
                    && rem >= 1
                    && enc_a_fits_row(&mut cws, &mut i, msg, rem)
                {
                    acted = true;
                }
            }

            if !acted {
                // Branch 6: in B and char fits.
                if cset == Subset::B
                    && in_set_b(msg[i])
                    && rem >= 1
                    && enc_b_fits_row(&mut cws, &mut i, msg, rem)
                {
                    acted = true;
                }
            }

            if !acted {
                // Branch 7: in C and we have a digit pair (or FN1).
                if cset == Subset::C && remnums >= 2 && rem >= 1 {
                    if msg[i] == FN1 {
                        push_c_fn1(&mut cws);
                        i += 1;
                    } else {
                        push_c_pair(&mut cws, msg[i], msg[i + 1]);
                        i += 2;
                    }
                    acted = true;
                }
            }

            if !acted {
                end_of_row = true;
            }
        }

        // Decide whether this is the last row.
        let j = cws.len();
        let rem_now = (c as i32 + 3) - (j % row_width) as i32;
        let could_be_last = r > 1 && i == msg.len() && rem_now >= 2;
        if could_be_last {
            pad_row(&mut cws, &mut cset, (rem_now - 2) as usize);
            cws.push(0); // k1 slot
            cws.push(0); // k2 slot
            cws.push(0); // row check slot
            push_seta(&mut cws, STP);
            last_row = true;
        } else {
            pad_row(&mut cws, &mut cset, rem_now as usize);
            cws.push(0); // row check slot
            push_seta(&mut cws, STP);
            r += 1;
        }
    }
    let total_rows = r;

    // k1, k2 over the entire message (with FN1 represented as ASCII 29
    // when it is not the leading character).
    let mut chkmsg: Vec<i32> = Vec::with_capacity(msg.len());
    for (idx, &ch) in msg.iter().enumerate() {
        if ch >= 0 {
            chkmsg.push(ch);
        }
        if ch == FN1 && idx != 0 {
            chkmsg.push(29);
        }
    }
    let mut k1: i32 = 0;
    let mut k2: i32 = 0;
    for (i, &ch) in chkmsg.iter().enumerate() {
        let t1 = (ch * i as i32).rem_euclid(86);
        let t2 = (t1 + ch).rem_euclid(86);
        k1 = (k1 + t2).rem_euclid(86);
        k2 = (k2 + t1).rem_euclid(86);
    }

    // abmap = [64..95] ∪ [0..15] ∪ [26..63] (86 entries). cmap = identity.
    let abmap: [u32; 86] = {
        let mut a = [0u32; 86];
        for (v, slot) in a.iter_mut().enumerate().take(32) {
            *slot = (64 + v) as u32;
        }
        for (v, slot) in a.iter_mut().skip(32).take(16).enumerate() {
            *slot = v as u32;
        }
        for (v, slot) in a.iter_mut().skip(48).take(38).enumerate() {
            *slot = (26 + v) as u32;
        }
        a
    };
    let map = |use_cmap: bool, v: i32| -> u32 {
        if use_cmap {
            v as u32
        } else {
            abmap[v as usize]
        }
    };

    // k1 at cws[n-4], k2 at cws[n-3]. The choice of abmap vs cmap follows
    // whichever subset the very last data char was encoded in (cmap iff C).
    let n = cws.len();
    let last_uses_c = cset == Subset::C;
    cws[n - 4] = map(last_uses_c, k1);
    cws[n - 3] = map(last_uses_c, k2);

    // Row indicators. Row 0 at cws[2]; row i (>=1) at cws[i*row_width+2].
    // The map choice for each row uses the preceding codeword (cws[pos-1]);
    // if that codeword is 99 (swc), data is in C, so cmap; else abmap.
    let row0_uses_c = cws[1] == 99;
    cws[2] = map(row0_uses_c, total_rows as i32 - 2);
    for i in 1..total_rows {
        let pos = (i as usize) * row_width + 2;
        let uses_c = cws[pos - 1] == 99;
        cws[pos] = map(uses_c, i as i32 + 42);
    }

    // Row check digit at slot c+3 (one before STP). Computed as the
    // standard Code 128 mod-103 check over the row's first c+4 codewords.
    for row in 0..total_rows {
        let start = row as usize * row_width;
        let mut csum: u32 = cws[start];
        for k in 1..(row_width - 1) {
            csum = csum.wrapping_add(cws[start + k].wrapping_mul(k as u32));
        }
        cws[start + row_width - 2] = csum % 103;
    }

    Ok(cws)
}

/// Run-length patterns for each codablockf codeword (BWIPP `codablockf_encs`).
/// Entries 0..103 are identical to Code 128's per-codeword 11-module patterns
/// in run-length form ("212222" = bar(2) space(1) bar(2) space(2) bar(2)
/// space(2)). Entry 104 is the 13-module stop ("2331112"; 4 bars + 3 spaces,
/// ending with the 2-wide termination bar).
#[rustfmt::skip]
const ENCS: &[&str] = &[
    "212222", "222122", "222221", "121223", "121322", "131222", "122213", "122312",
    "132212", "221213", "221312", "231212", "112232", "122132", "122231", "113222",
    "123122", "123221", "223211", "221132", "221231", "213212", "223112", "312131",
    "311222", "321122", "321221", "312212", "322112", "322211", "212123", "212321",
    "232121", "111323", "131123", "131321", "112313", "132113", "132311", "211313",
    "231113", "231311", "112133", "112331", "132131", "113123", "113321", "133121",
    "313121", "211331", "231131", "213113", "213311", "213131", "311123", "311321",
    "331121", "312113", "312311", "332111", "314111", "221411", "431111", "111224",
    "111422", "121124", "121421", "141122", "141221", "112214", "112412", "122114",
    "122411", "142112", "142211", "241211", "221114", "413111", "241112", "134111",
    "111242", "121142", "121241", "114212", "124112", "124211", "411212", "421112",
    "421211", "212141", "214121", "412121", "111143", "111341", "131141", "114113",
    "114311", "411113", "411311", "113141", "114131", "311141", "411131", "211412",
    "2331112",
];

/// Convert a flat codeword array (`row_width = columns + 5` codewords per row)
/// into a [`StackedPattern`] by emitting each codeword's run-length encoding
/// in BWIPP order. Each non-stop entry contributes 6 run-length cells
/// (3 bars + 3 spaces); the stop contributes 7 (4 bars + 3 spaces), ending
/// each row on a bar.
fn codewords_to_stacked(cws: &[u32], columns: usize, text: Option<String>) -> StackedPattern {
    let row_width = columns + 5;
    debug_assert!(cws.len() % row_width == 0);
    let mut rows = Vec::with_capacity(cws.len() / row_width);
    for row in cws.chunks(row_width) {
        let mut bars: Vec<u8> = Vec::with_capacity(row.len() * 6 + 1);
        for &cw in row {
            for ch in ENCS[cw as usize].chars() {
                let n = ch.to_digit(10).expect("ENCS entry must be all digits") as u8;
                bars.push(n);
            }
        }
        rows.push(LinearPattern { bars, text: None });
    }
    StackedPattern { rows, text }
}

/// Encode a Codablock-F symbol from ASCII text. The number of data columns
/// per row is read from `opts.extras` (key `"columns"`); if absent or
/// out of range, defaults to 8 (matching bwip-js).
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// let mut opts = Options::default();
/// opts.extras.push(("columns".into(), "12".into())); // wider rows
/// let svg = render_svg(Symbology::CodablockF, "Hello, world!", &opts).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode(data: &str, opts: &Options) -> Result<StackedPattern, Error> {
    if data.is_empty() {
        return Err(Error::InvalidData(
            "Codablock-F payload must not be empty".into(),
        ));
    }
    for c in data.chars() {
        if (c as u32) > 127 {
            return Err(Error::InvalidData(format!(
                "Codablock-F only supports ASCII; got {c:?}"
            )));
        }
    }
    let columns = opts
        .extras
        .iter()
        .find(|(k, _)| k == "columns")
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .filter(|c| (4..=62).contains(c))
        .unwrap_or(8);

    let msg: Vec<i32> = data.bytes().map(|b| b as i32).collect();
    let cws = codewords(&msg, columns).map_err(|s| Error::InvalidData((*s).to_string()))?;
    Ok(codewords_to_stacked(&cws, columns, Some(data.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_msg(s: &str) -> Vec<i32> {
        s.bytes().map(|b| b as i32).collect()
    }

    /// Golden values captured from bwip-js's `debugcws` output via
    /// `node-sidecar/oracle-codablockf.js`. Each tuple is
    /// `(input_text, columns, expected_cws)`.
    fn golden() -> &'static [(&'static str, usize, &'static [u32])] {
        &[
            (
                "AB",
                8,
                &[
                    103, 100, 64, 33, 34, 99, 100, 99, 100, 99, 100, 101, 104, 103, 100, 11, 99,
                    100, 99, 100, 99, 100, 89, 44, 13, 104,
                ],
            ),
            (
                "HELLO",
                8,
                &[
                    103, 100, 64, 40, 37, 44, 44, 47, 99, 100, 99, 77, 104, 103, 100, 11, 99, 100,
                    99, 100, 99, 100, 83, 55, 69, 104,
                ],
            ),
            (
                "12345",
                8,
                &[
                    103, 99, 0, 12, 34, 100, 21, 99, 100, 99, 100, 58, 104, 103, 100, 11, 99, 100,
                    99, 100, 99, 100, 65, 68, 37, 104,
                ],
            ),
            (
                "abc123ABC",
                8,
                &[
                    103, 100, 64, 65, 66, 67, 17, 18, 19, 33, 34, 82, 104, 103, 100, 11, 35, 99,
                    100, 99, 100, 99, 4, 50, 37, 104,
                ],
            ),
            (
                "BWIPP-RS",
                8,
                &[
                    103, 100, 64, 34, 55, 41, 48, 48, 13, 50, 51, 74, 104, 103, 100, 11, 99, 100,
                    99, 100, 99, 100, 85, 91, 35, 104,
                ],
            ),
            (
                "1234567890",
                8,
                &[
                    103, 99, 0, 12, 34, 56, 78, 90, 100, 99, 100, 14, 104, 103, 100, 11, 99, 100,
                    99, 100, 99, 100, 65, 56, 20, 104,
                ],
            ),
        ]
    }

    #[test]
    fn codewords_match_bwip_js_oracle() {
        for &(text, c, expected) in golden() {
            let got = codewords(&to_msg(text), c)
                .unwrap_or_else(|e| panic!("codewords({text:?}, {c}) failed: {e}"));
            assert_eq!(
                got.as_slice(),
                expected,
                "codewords({text:?}, {c}) mismatch:\n got:    {got:?}\n wanted: {expected:?}",
            );
        }
    }

    #[test]
    fn rendered_row_widths_match() {
        // Every row should contribute (columns + 4) * 11 + 13 modules — the
        // c+4 ordinary 11-module codewords plus one 13-module stop. For
        // columns=8 that is 12*11 + 13 = 145 modules per row.
        for &(text, c, _) in golden() {
            let opts = Options::default();
            let mut opts = opts;
            opts.extras.push(("columns".into(), c.to_string()));
            // Stage 11.A8c — input + column-count echo so a
            // regression points at the specific golden row that
            // stopped encoding.
            let stacked = encode(text, &opts).unwrap_or_else(|e| {
                panic!("codablockf encode(text={text:?}, columns={c}) failed: {e:?}");
            });
            let expected_width = ((c + 4) * 11 + 13) as u32;
            for (i, row) in stacked.rows.iter().enumerate() {
                let w = row.total_width();
                assert_eq!(
                    w, expected_width,
                    "{text:?} row {i} width = {w}, want {expected_width}"
                );
            }
        }
    }

    /// Stage 11.A8c — pin the empty-payload and non-ASCII rejection arms
    /// at lines 584-594 of `encode`. The legacy assertion was just
    /// `is_err()` which survives diagnostic-string drift, swap of the two
    /// guards' messages, and iteration-order mutants (e.g. `chars().rev()`).
    ///
    /// Mutations to catch:
    ///   - `data.is_empty()` → `!data.is_empty()`: empty would succeed
    ///     (or fall through to a different error path).
    ///   - Empty diagnostic string drift (e.g. swap "must not be empty"
    ///     with the ASCII message).
    ///   - `(c as u32) > 127` → `>= 127`: pure-ASCII payload with '~' (126)
    ///     would now reject; we anchor with 'é' (233) which still fires.
    ///     The boundary direction is anchored by the ASCII sanity case.
    ///   - `chars()` → `chars().rev()`: the reported char in `{c:?}`
    ///     would be the LAST non-ASCII char, not the first. We pin
    ///     "café…ü" so the first non-ASCII is 'é' and the last is 'ü' —
    ///     a rev-mutant reports 'ü' instead.
    #[test]
    fn encode_rejects_empty_and_non_ascii() {
        // Arm 1: empty payload rejects with the empty-specific diagnostic.
        // Stage 11.A8c (cont) — switch from `let-else` to `match` so the
        // failing variant is echoed in the panic, killing mutants that
        // route empty-payload rejection through a non-InvalidData variant.
        let msg = match encode("", &Options::default()).expect_err("empty must reject") {
            Error::InvalidData(m) => m,
            other => panic!(
                "empty payload (\"\") must reject as InvalidData; got {other:?} (mutation re-routed empty-guard to wrong error variant)"
            ),
        };
        assert!(
            msg.contains("Codablock-F") && msg.contains("must not be empty"),
            "empty-payload diagnostic must mention Codablock-F + 'must not be empty'; got {msg:?}"
        );
        assert!(
            !msg.contains("ASCII"),
            "empty-payload diagnostic must not leak the ASCII message; got {msg:?}"
        );

        // Arm 2: 'é' alone rejects via the ASCII guard, reporting the char.
        let msg = match encode("é", &Options::default()).expect_err("non-ASCII must reject") {
            Error::InvalidData(m) => m,
            other => panic!(
                "non-ASCII payload \"é\" must reject as InvalidData; got {other:?} (mutation re-routed ASCII guard)"
            ),
        };
        assert!(
            msg.contains("only supports ASCII"),
            "non-ASCII diagnostic must mention 'only supports ASCII'; got {msg:?}"
        );
        assert!(
            msg.contains("'é'"),
            "non-ASCII diagnostic must mention the offending char 'é'; got {msg:?}"
        );

        // Arm 2 (iteration-order pin): "caféü" has TWO non-ASCII chars.
        // The first-encountered is 'é'; a `.rev()` mutant would report 'ü'.
        let msg =
            match encode("caféü", &Options::default()).expect_err("multi non-ASCII must reject") {
                Error::InvalidData(m) => m,
                other => panic!(
                    "multi non-ASCII payload \"caféü\" must reject as InvalidData; got {other:?}"
                ),
            };
        assert!(
            msg.contains("'é'"),
            "diagnostic must report the FIRST non-ASCII char 'é' (not 'ü'); got {msg:?}"
        );
        assert!(
            !msg.contains("'ü'"),
            "diagnostic must not report 'ü' (would indicate reverse iteration); got {msg:?}"
        );

        // ASCII boundary: '~' (126) is pure ASCII and must NOT trip the
        // non-ASCII arm. A `>= 127` mutant would still pass this; a
        // `< 127` (negation) mutant would reject ASCII entirely.
        assert!(
            encode("~", &Options::default()).is_ok(),
            "ASCII char '~' (126) must encode successfully"
        );
        assert!(
            encode("ABC", &Options::default()).is_ok(),
            "pure-ASCII 'ABC' must encode successfully (kills predicate-negation mutants)"
        );
    }

    /// Stage 11.A8c — pin `encode`'s columns-option parsing arm at
    /// lines 596-602. The `.filter(|c| (4..=62).contains(c)).unwrap_or(8)`
    /// chain silently falls back to 8 for out-of-range OR unparseable
    /// inputs. End-to-end goldens pin specific column counts but the
    /// fall-back path was never directly exercised.
    ///
    /// Row width formula: `(columns + 4) * 11 + 13` modules.
    ///   - columns=8 (default): 12*11 + 13 = 145.
    ///   - columns=4 (lower bound): 8*11 + 13 = 101.
    ///   - columns=62 (upper bound): 66*11 + 13 = 739.
    ///
    /// Mutations to catch:
    ///   - `(4..=62)` → `(4..62)`: rejects columns=62.
    ///   - `(4..=62)` → `(5..=62)`: rejects columns=4.
    ///   - `.unwrap_or(8)` → `.unwrap_or(12)`: default falls back to
    ///     wrong width.
    #[test]
    fn encode_columns_option_default_boundaries_and_fallback() {
        // Default (no columns option): rows are 145 modules wide.
        let stacked = encode("AB", &Options::default()).expect("default columns");
        for row in &stacked.rows {
            assert_eq!(
                row.total_width(),
                145,
                "default columns (=8) → row width 145; mutant that changes the default break this"
            );
        }

        // Lower boundary columns=4: rows are 101 modules wide.
        let mut opts = Options::default();
        opts.extras.push(("columns".into(), "4".into()));
        let stacked = encode("AB", &opts).expect("columns=4 (lower bound)");
        for row in &stacked.rows {
            assert_eq!(
                row.total_width(),
                101,
                "columns=4 → row width 101; mutant `(5..=62)` would reject 4 → default 145"
            );
        }

        // Upper boundary columns=62: rows are 739 modules wide.
        // Payload needs to be reasonable size for column=62 not to error.
        let payload = "A".repeat(60);
        let mut opts = Options::default();
        opts.extras.push(("columns".into(), "62".into()));
        let stacked = encode(&payload, &opts).expect("columns=62 (upper bound)");
        for row in &stacked.rows {
            assert_eq!(
                row.total_width(),
                739,
                "columns=62 → row width 739; mutant `(4..62)` would reject 62 → default 145"
            );
        }

        // Out-of-range below (3): falls back to default 8 → width 145.
        let mut opts = Options::default();
        opts.extras.push(("columns".into(), "3".into()));
        let stacked = encode("AB", &opts).expect("columns=3 falls back to default");
        for row in &stacked.rows {
            assert_eq!(
                row.total_width(),
                145,
                "columns=3 < 4 falls back to 8 → width 145"
            );
        }

        // Out-of-range above (63): falls back to default 8 → width 145.
        let mut opts = Options::default();
        opts.extras.push(("columns".into(), "63".into()));
        let stacked = encode("AB", &opts).expect("columns=63 falls back to default");
        for row in &stacked.rows {
            assert_eq!(
                row.total_width(),
                145,
                "columns=63 > 62 falls back to 8 → width 145"
            );
        }

        // Parse failure (non-numeric): falls back to default 8.
        let mut opts = Options::default();
        opts.extras.push(("columns".into(), "abc".into()));
        let stacked = encode("AB", &opts).expect("columns=abc falls back to default");
        for row in &stacked.rows {
            assert_eq!(
                row.total_width(),
                145,
                "columns=abc parse-fail falls back to 8 → width 145"
            );
        }
    }

    /// Kills the `seta` match-arm mutants on lines 43-51 (one per
    /// arm). Each arm maps a specific token to a specific codeword;
    /// the existing end-to-end goldens only exercise the printable
    /// ASCII 'A'-'Z' range, leaving the control-byte and sentinel
    /// arms uncovered. Direct unit tests pin every arm.
    #[test]
    fn seta_covers_every_match_arm() {
        // Control range 0..=31 → tok + 64.
        assert_eq!(seta(0), Some(64));
        assert_eq!(seta(31), Some(95));
        // Printable range 32..=95 → tok - 32.
        assert_eq!(seta(32), Some(0));
        assert_eq!(seta(b' ' as i32), Some(0));
        assert_eq!(seta(b'A' as i32), Some(33));
        assert_eq!(seta(95), Some(63));
        // Specific sentinel codewords.
        assert_eq!(seta(FN3), Some(96));
        assert_eq!(seta(FN2), Some(97));
        assert_eq!(seta(SFT), Some(98));
        assert_eq!(seta(SWC), Some(99));
        assert_eq!(seta(SWB), Some(100));
        assert_eq!(seta(FN4), Some(101));
        assert_eq!(seta(FN1), Some(102));
        assert_eq!(seta(STA), Some(103));
        assert_eq!(seta(STP), Some(104));
        // Out-of-range returns None.
        assert_eq!(seta(96), None);
        assert_eq!(seta(127), None);
        assert_eq!(seta(SWA), None); // SWA is set-B-only sentinel
    }

    /// Kills the `setb` match-arm mutants on lines 61-69. Same
    /// shape as the seta test, mirrored for set B's mapping.
    #[test]
    fn setb_covers_every_match_arm() {
        // Printable range 32..=127 → tok - 32.
        assert_eq!(setb(32), Some(0));
        assert_eq!(setb(b' ' as i32), Some(0));
        assert_eq!(setb(b'a' as i32), Some(65));
        assert_eq!(setb(127), Some(95));
        // Specific sentinel codewords.
        assert_eq!(setb(FN3), Some(96));
        assert_eq!(setb(FN2), Some(97));
        assert_eq!(setb(SFT), Some(98));
        assert_eq!(setb(SWC), Some(99));
        assert_eq!(setb(FN4), Some(100));
        assert_eq!(setb(SWA), Some(101));
        assert_eq!(setb(FN1), Some(102));
        assert_eq!(setb(STA), Some(103));
        assert_eq!(setb(STP), Some(104));
        // Out-of-range returns None.
        assert_eq!(setb(0), None);
        assert_eq!(setb(31), None);
        assert_eq!(setb(SWB), None); // SWB is set-A-only sentinel
    }

    /// Kills the `in_set_a`/`in_set_b` function-replacement mutants
    /// at lines 75/79 (`replace -> bool with false`) and the
    /// `anotb`/`bnota` mutants at lines 91/95 (function-replacement,
    /// `&& with ||`, `delete !`).
    ///
    /// Pinning specific true/false return values for each predicate
    /// at characters that distinguish set A vs set B:
    ///   * lowercase 'a' (=97): in set B only.
    ///   * control char NUL (=0): in set A only.
    ///   * digit '5' (=53): in both A and B.
    ///   * sentinel SWA (=-1): in set B only.
    ///   * sentinel SWB (=-2): in set A only.
    #[test]
    fn set_membership_predicates_distinguish_a_and_b() {
        // 'a' = 97 → in B (32..=127 arm), not in A (32..=95 stops at 95).
        assert!(in_set_b(b'a' as i32));
        assert!(!in_set_a(b'a' as i32));
        assert!(bnota(b'a' as i32));
        assert!(!anotb(b'a' as i32));

        // NUL = 0 → in A (control 0..=31 arm), not in B (B starts at 32).
        assert!(in_set_a(0));
        assert!(!in_set_b(0));
        assert!(anotb(0));
        assert!(!bnota(0));

        // '5' = 53 → in both A and B (printable range covers both).
        assert!(in_set_a(b'5' as i32));
        assert!(in_set_b(b'5' as i32));
        assert!(!anotb(b'5' as i32));
        assert!(!bnota(b'5' as i32));

        // SWA sentinel → in B only (set-B's SWA arm).
        assert!(in_set_b(SWA));
        assert!(!in_set_a(SWA));

        // SWB sentinel → in A only (set-A's SWB arm).
        assert!(in_set_a(SWB));
        assert!(!in_set_b(SWB));
    }

    /// Stage 11.A8c — pin `push_c_pair`'s digit-pair arithmetic.
    /// The helper is used in 3 places to compute the subset-C
    /// codeword from two ASCII digit bytes: `(a-48)*10 + (b-48)`.
    /// Existing tests exercise it indirectly through end-to-end
    /// goldens, but a mutant like `* 10` → `* 100` (codeword 10×
    /// larger), `* 10` → `+ 10` (collapses to small range), or
    /// `- 48` → `- 47` (off-by-one base shift) is easier to localise
    /// with a direct unit test.
    ///
    /// Hand-computed (a, b are ASCII bytes):
    ///   - push_c_pair('0', '0') → 0
    ///   - push_c_pair('0', '5') → 5
    ///   - push_c_pair('1', '2') → 12
    ///   - push_c_pair('5', '0') → 50
    ///   - push_c_pair('9', '9') → 99
    #[test]
    fn push_c_pair_computes_decimal_pair_correctly() {
        let mut cws: Vec<u32> = Vec::new();
        push_c_pair(&mut cws, b'0' as i32, b'0' as i32);
        push_c_pair(&mut cws, b'0' as i32, b'5' as i32);
        push_c_pair(&mut cws, b'1' as i32, b'2' as i32);
        push_c_pair(&mut cws, b'5' as i32, b'0' as i32);
        push_c_pair(&mut cws, b'9' as i32, b'9' as i32);
        assert_eq!(
            cws,
            vec![0, 5, 12, 50, 99],
            "push_c_pair digit-pair arithmetic regressed; check `(a-48)*10 + (b-48)`"
        );
    }

    /// Stage 11.A8c — pin `push_seta` / `push_setb` happy-path
    /// behaviour. They're thin wrappers around `seta` / `setb` that
    /// panic on None. Existing tests pin seta/setb's per-arm
    /// mappings; this test exercises the push side.
    ///
    /// push_seta(' ' = 32) should push seta(32) = Some(0) → cws gets 0.
    /// push_setb('a' = 97) should push setb(97) = Some(65) → cws gets 65.
    /// push_seta(FN1) should push 102.
    /// push_setb(FN1) should also push 102.
    /// Multiple calls append (not replace).
    #[test]
    fn push_seta_and_push_setb_append_correct_codewords() {
        let mut cws_a: Vec<u32> = Vec::new();
        push_seta(&mut cws_a, b' ' as i32);
        push_seta(&mut cws_a, FN1);
        push_seta(&mut cws_a, b'A' as i32); // A → 65-32 = 33.
        assert_eq!(
            cws_a,
            vec![0, 102, 33],
            "push_seta must append seta() lookups in order"
        );

        let mut cws_b: Vec<u32> = Vec::new();
        push_setb(&mut cws_b, b'a' as i32); // a (97) → 97-32 = 65.
        push_setb(&mut cws_b, FN1);
        push_setb(&mut cws_b, b' ' as i32); // space → 0.
        assert_eq!(
            cws_b,
            vec![65, 102, 0],
            "push_setb must append setb() lookups in order"
        );
    }

    /// Stage 11.A8c — pin `pad_row` alternation between SWC (99) and
    /// SWB (100). The helper fills the remaining slots of a row with
    /// alternating switch codewords, starting from `cset` and toggling
    /// between C and B after each push. Mutations on the cset
    /// transitions (e.g. always-C) or codeword constants would slip
    /// through end-to-end tests if the symbol still encodes valid bars
    /// for the corpus inputs.
    ///
    /// Hand-computed: from A/B → SWC (99) → cset=C, from C → SWB (100)
    /// → cset=B. Continuing from B → SWC → cset=C, etc.
    #[test]
    fn pad_row_alternates_switch_codewords() {
        // No-op for n=0.
        let mut cws: Vec<u32> = Vec::new();
        let mut cset = Subset::A;
        pad_row(&mut cws, &mut cset, 0);
        assert_eq!(cws, Vec::<u32>::new());
        assert_eq!(cset, Subset::A, "n=0 must not change cset");

        // From A, n=1 → push 99 (SWC), cset=C.
        let mut cws: Vec<u32> = Vec::new();
        let mut cset = Subset::A;
        pad_row(&mut cws, &mut cset, 1);
        assert_eq!(cws, vec![99]);
        assert_eq!(cset, Subset::C);

        // From A, n=4 → 99, 100, 99, 100; cset oscillates A→C→B→C→B.
        let mut cws: Vec<u32> = Vec::new();
        let mut cset = Subset::A;
        pad_row(&mut cws, &mut cset, 4);
        assert_eq!(cws, vec![99, 100, 99, 100]);
        assert_eq!(cset, Subset::B, "after 4 toggles from A: C→B→C→B");

        // From B, n=1 → push 99 (SWC), cset=C.
        let mut cws: Vec<u32> = Vec::new();
        let mut cset = Subset::B;
        pad_row(&mut cws, &mut cset, 1);
        assert_eq!(cws, vec![99]);
        assert_eq!(cset, Subset::C);

        // From C, n=1 → push 100 (SWB), cset=B.
        let mut cws: Vec<u32> = Vec::new();
        let mut cset = Subset::C;
        pad_row(&mut cws, &mut cset, 1);
        assert_eq!(cws, vec![100]);
        assert_eq!(cset, Subset::B);

        // From C, n=3 → 100, 99, 100; cset C→B→C→B.
        let mut cws: Vec<u32> = Vec::new();
        let mut cset = Subset::C;
        pad_row(&mut cws, &mut cset, 3);
        assert_eq!(cws, vec![100, 99, 100]);
        assert_eq!(cset, Subset::B);

        // Appends to existing cws (doesn't replace).
        let mut cws: Vec<u32> = vec![42, 7];
        let mut cset = Subset::A;
        pad_row(&mut cws, &mut cset, 1);
        assert_eq!(cws, vec![42, 7, 99]);
    }

    /// Stage 11.A8c — pin the three set-membership predicates
    /// `in_set_a` / `in_set_b` / `in_set_c` plus the `anotb` and
    /// `bnota` exclusion combinators. These drive every subset-switch
    /// decision in `codewords()` and any swap/inversion mutation would
    /// silently reroute encoder paths without breaking the high-level
    /// goldens.
    ///
    /// Anchor inputs picked to distinguish every (A, B, C) membership
    /// combination plus the A-only/B-only sentinels (SWB/SWA):
    ///
    /// | input    | A   | B   | C   | anotb | bnota |
    /// |----------|-----|-----|-----|-------|-------|
    /// | 5 (ctrl) | yes | no  | no  | yes   | no    |
    /// | 'A' (65) | yes | yes | no  | no    | no    |
    /// | '0' (48) | yes | yes | yes | no    | no    |
    /// | 'a' (97) | no  | yes | no  | no    | yes   |
    /// | DEL(127) | no  | yes | no  | no    | yes   |
    /// | SWA      | no  | yes | yes | no    | yes   |
    /// | SWB      | yes | no  | yes | yes   | no    |
    /// | SWC      | yes | yes | no  | no    | no    |
    /// | FN1      | yes | yes | yes | no    | no    |
    /// | FN4      | yes | yes | no  | no    | no    |
    /// | STA      | yes | yes | yes | no    | no    |
    /// | STP      | yes | yes | yes | no    | no    |
    /// | 200      | no  | no  | no  | no    | no    |
    ///
    /// Mutations to catch (sample):
    ///   - `in_set_a` body inversion → ctrl chars and SWB flip.
    ///   - `anotb`/`bnota` `&&` → `||` flips overlap cases (e.g. 'A').
    ///   - `bnota` losing the `!` → 'A' (in both) wrongly true.
    ///   - `in_set_c` `0..=9` → `0..9` would reject '9' (57).
    #[test]
    fn set_membership_and_exclusion_predicates() {
        // ---- in_set_a ----------------------------------------------------
        assert!(in_set_a(5), "ctrl 5 → A");
        assert!(in_set_a(b'A' as i32), "'A' → A");
        assert!(in_set_a(b'0' as i32), "'0' → A");
        assert!(!in_set_a(b'a' as i32), "lowercase → !A");
        assert!(!in_set_a(127), "DEL → !A (range stops at 95)");
        assert!(!in_set_a(SWA), "SWA → !A");
        assert!(in_set_a(SWB), "SWB → A");
        assert!(in_set_a(SWC), "SWC → A");
        assert!(in_set_a(FN1), "FN1 → A");
        assert!(in_set_a(FN4), "FN4 → A");
        assert!(in_set_a(STA), "STA → A");
        assert!(in_set_a(STP), "STP → A");
        assert!(!in_set_a(200), "out-of-range positive → !A");

        // ---- in_set_b ----------------------------------------------------
        assert!(!in_set_b(5), "ctrl 5 → !B");
        assert!(in_set_b(b'A' as i32), "'A' → B");
        assert!(in_set_b(b'0' as i32), "'0' → B");
        assert!(in_set_b(b'a' as i32), "lowercase → B");
        assert!(in_set_b(127), "DEL → B (range goes through 127)");
        assert!(in_set_b(SWA), "SWA → B");
        assert!(!in_set_b(SWB), "SWB → !B");
        assert!(in_set_b(SWC), "SWC → B");
        assert!(in_set_b(FN1), "FN1 → B");
        assert!(in_set_b(FN4), "FN4 → B");
        assert!(in_set_b(STA), "STA → B");
        assert!(in_set_b(STP), "STP → B");
        assert!(!in_set_b(200), "out-of-range positive → !B");

        // ---- in_set_c ----------------------------------------------------
        assert!(!in_set_c(5), "ctrl 5 → !C");
        assert!(!in_set_c(b'A' as i32), "'A' → !C");
        assert!(in_set_c(b'0' as i32), "'0' → C");
        assert!(in_set_c(b'9' as i32), "'9' → C (rejects 0..9 mutation)");
        assert!(!in_set_c(b'/' as i32), "'/' (47) → !C (just before '0')");
        assert!(!in_set_c(b':' as i32), "':' (58) → !C (just after '9')");
        assert!(!in_set_c(b'a' as i32), "lowercase → !C");
        assert!(in_set_c(SWA), "SWA → C");
        assert!(in_set_c(SWB), "SWB → C");
        assert!(!in_set_c(SWC), "SWC → !C");
        assert!(in_set_c(FN1), "FN1 → C");
        assert!(!in_set_c(FN2), "FN2 → !C");
        assert!(!in_set_c(FN3), "FN3 → !C");
        assert!(!in_set_c(FN4), "FN4 → !C");
        assert!(!in_set_c(SFT), "SFT → !C");
        assert!(in_set_c(STA), "STA → C");
        assert!(in_set_c(STP), "STP → C");

        // ---- anotb (A but not B) ----------------------------------------
        assert!(anotb(5), "ctrl 5 ∈ A\\B");
        assert!(!anotb(b'A' as i32), "'A' ∈ A∩B → !anotb");
        assert!(!anotb(b'0' as i32), "'0' ∈ A∩B → !anotb");
        assert!(!anotb(b'a' as i32), "lowercase ∉ A → !anotb");
        assert!(!anotb(SWA), "SWA ∉ A → !anotb");
        assert!(anotb(SWB), "SWB ∈ A only → anotb");
        assert!(!anotb(SWC), "SWC ∈ A∩B → !anotb");
        assert!(!anotb(FN1), "FN1 ∈ A∩B → !anotb");
        assert!(!anotb(200), "200 ∉ either → !anotb");

        // ---- bnota (B but not A) ----------------------------------------
        assert!(!bnota(5), "ctrl 5 ∉ B → !bnota");
        assert!(!bnota(b'A' as i32), "'A' ∈ A∩B → !bnota");
        assert!(!bnota(b'0' as i32), "'0' ∈ A∩B → !bnota");
        assert!(bnota(b'a' as i32), "lowercase ∈ B\\A");
        assert!(bnota(127), "DEL ∈ B\\A");
        assert!(bnota(SWA), "SWA ∈ B only → bnota");
        assert!(!bnota(SWB), "SWB ∉ B → !bnota");
        assert!(!bnota(SWC), "SWC ∈ A∩B → !bnota");
        assert!(!bnota(FN1), "FN1 ∈ A∩B → !bnota");
        assert!(!bnota(200), "200 ∉ either → !bnota");
    }

    /// Kills the `push_c_fn1 137:5` mutant (`replace body with ()`).
    /// If the body becomes a no-op, the function fails to push the
    /// 102 codeword that marks FN1 inside subset C.
    #[test]
    fn push_c_fn1_emits_the_fn1_codeword_in_subset_c() {
        let mut cws: Vec<u32> = Vec::new();
        push_c_fn1(&mut cws);
        assert_eq!(
            cws,
            vec![102],
            "push_c_fn1 must emit codeword 102 (FN1 in set C)"
        );

        // Calling it again appends — the function is `push`, not `set`.
        push_c_fn1(&mut cws);
        assert_eq!(cws, vec![102, 102]);
    }

    /// Kills the `numsscr` arithmetic mutants at lines 107 + 115
    /// (`- with /`, `== with !=`, `% with /`, `% with +`). These
    /// compute the digit-pair counter for subset C runs; the
    /// existing tests exercise short payloads where the counter
    /// stays within one iteration, leaving the alternating-s parity
    /// and the FN1-tracking branches uncovered.
    #[test]
    fn numsscr_counts_digit_pairs_with_fn1_handling() {
        // Pure 4-digit run: 4 digits → n=4, s=4 (BWIPP semantics:
        // each digit increments both n and s; pairs are formed
        // greedily).
        let msg: Vec<i32> = "1234".bytes().map(|b| b as i32).collect();
        assert_eq!(numsscr(&msg, 0), (4, 4));

        // FN1 in middle (at even s parity): triggers the s%2==0
        // half-pair increment, then the standard n+=1, s+=1 runs.
        let msg: Vec<i32> = vec![b'1' as i32, b'2' as i32, FN1, b'3' as i32, b'4' as i32];
        // Walk: i=0 '1' → n=1, s=1. i=1 '2' → n=2, s=2.
        // i=2 FN1: s%2==0 (s=2 even) → s+=1=3, then n+=1=3, s+=1=4.
        // i=3 '3' → n=4, s=5. i=4 '4' → n=5, s=6.
        assert_eq!(numsscr(&msg, 0), (5, 6));

        // Non-set-C interruption stops the run.
        let msg: Vec<i32> = vec![b'1' as i32, b'2' as i32, b'A' as i32, b'3' as i32];
        // Walk: i=0 '1' → (1, 1). i=1 '2' → (2, 2). i=2 'A': not in
        // set C → break.
        assert_eq!(numsscr(&msg, 0), (2, 2));

        // Kills the `107:28 - with /` mutant: the FN4-predecessor
        // guard checks `msg[p - 1]`, not `msg[p / 1] = msg[p]`. If
        // the mutant flips the index, the guard fires on the current
        // byte instead of the previous one.
        //
        // For msg = [FN4, '1', '2', '3'] at p=1:
        //   Original: msg[0]=FN4 → break, return (0, 0).
        //   Mutant:   msg[1]='1' (not FN4) → continue scanning,
        //             return (3, 3) (the next three digits).
        let msg: Vec<i32> = vec![FN4, b'1' as i32, b'2' as i32, b'3' as i32];
        assert_eq!(
            numsscr(&msg, 1),
            (0, 0),
            "FN4-preceded numeric scan must stop at the predecessor guard"
        );
        // Sanity bracket: p=0 falls through to in_set_c(FN4) = false →
        // break → (0, 0); p=2 sees msg[1]='1' (not FN4) → scans the
        // tail digits.
        assert_eq!(numsscr(&msg, 0), (0, 0));
        assert_eq!(numsscr(&msg, 2), (2, 2));
    }

    /// Kills the `enc_a_fits_row` boundary mutants on lines 172-182:
    /// `<= with >` (rem <= 2), `== with !=` (FN4 / rem == 2), `&& with ||`,
    /// `+ with - / *` (msg[*i + 1] indexing), `+= with -= / *=` (i += 2).
    ///
    /// Exercises every branch:
    ///   * rem=3, ordinary byte → push, i++, return true.
    ///   * rem=2, FN4 + byte ≤ 95 → push pair, i+=2, return true.
    ///   * rem=2, FN4 + byte > 95 → return false, i unchanged.
    ///   * rem=1, FN4 → return false, i unchanged (rem < 2).
    #[test]
    fn enc_a_fits_row_handles_fn4_tail() {
        // rem=3, ordinary byte 'A' (= 65): non-FN4 branch.
        let mut cws: Vec<u32> = Vec::new();
        let mut i: usize = 0;
        let msg = vec![b'A' as i32];
        assert!(enc_a_fits_row(&mut cws, &mut i, &msg, 3));
        assert_eq!(i, 1);
        assert_eq!(cws, vec![33]); // 'A' = 65 → 65 - 32 = 33

        // rem=2, FN4 + 'X' (=88, ≤ 95): fits, push 2 codewords.
        let mut cws: Vec<u32> = Vec::new();
        let mut i: usize = 0;
        let msg = vec![FN4, b'X' as i32];
        assert!(enc_a_fits_row(&mut cws, &mut i, &msg, 2));
        assert_eq!(i, 2);
        assert_eq!(cws, vec![101, 56]); // FN4=101, 'X'=88-32=56

        // rem=2, FN4 + lowercase 'a' (=97, > 95): doesn't fit.
        // Stage 11.A8c (cont) — descriptive label naming failure invariant.
        let mut cws: Vec<u32> = Vec::new();
        let mut i: usize = 0;
        let msg = vec![FN4, b'a' as i32];
        assert!(!enc_a_fits_row(&mut cws, &mut i, &msg, 2));
        assert_eq!(i, 0); // i unchanged on failure
        assert!(
            cws.is_empty(),
            "enc_a_fits_row(rem=2, FN4+'a') returned false → cws must remain empty (no partial mutation); got len={}",
            cws.len()
        );

        // rem=1, FN4: rem < 2 → fits=false, but also msg[*i]==FN4 so
        // we enter the FN4 branch. fits=(1==2) = false. Return false.
        let mut cws: Vec<u32> = Vec::new();
        let mut i: usize = 0;
        let msg = vec![FN4, b'A' as i32];
        assert!(!enc_a_fits_row(&mut cws, &mut i, &msg, 1));
        assert_eq!(i, 0);

        // Kills `172:17 && with ||` (and the matching mutant on
        // enc_b_fits_row): if the guard becomes
        // `rem <= 2 || msg[*i] == FN4`, then rem=3 with msg[0]==FN4
        // takes the FN4 sub-branch instead of falling through to the
        // ordinary push. With rem=3 the FN4 sub-branch fails (rem != 2)
        // and returns false, leaving cws empty.
        //
        // Original behavior with rem=3 + FN4 head: ordinary push
        // (since rem > 2), i+=1, return true.
        let mut cws: Vec<u32> = Vec::new();
        let mut i: usize = 0;
        let msg = vec![FN4, b'X' as i32];
        assert!(enc_a_fits_row(&mut cws, &mut i, &msg, 3));
        assert_eq!(i, 1);
        assert_eq!(cws, vec![101]); // FN4 = 101 in set A
    }

    /// Kills the `enc_b_fits_row` mutants on lines 188-193 — mirror
    /// of the enc_a_fits_row test but with the `>= 32` lower-bound
    /// check (set B's printable range starts at 32, not 0).
    #[test]
    fn enc_b_fits_row_handles_fn4_tail() {
        // rem=3, ordinary 'a' (=97): non-FN4 branch.
        let mut cws: Vec<u32> = Vec::new();
        let mut i: usize = 0;
        let msg = vec![b'a' as i32];
        assert!(enc_b_fits_row(&mut cws, &mut i, &msg, 3));
        assert_eq!(i, 1);
        assert_eq!(cws, vec![65]); // 'a' = 97 → 97 - 32 = 65

        // rem=2, FN4 + ' ' (=32, ≥ 32): fits.
        let mut cws: Vec<u32> = Vec::new();
        let mut i: usize = 0;
        let msg = vec![FN4, b' ' as i32];
        assert!(enc_b_fits_row(&mut cws, &mut i, &msg, 2));
        assert_eq!(i, 2);
        assert_eq!(cws, vec![100, 0]); // FN4=100 (set B), ' '=32-32=0

        // rem=2, FN4 + control byte 31 (< 32): doesn't fit.
        // Stage 11.A8c (cont) — descriptive label naming failure invariant.
        let mut cws: Vec<u32> = Vec::new();
        let mut i: usize = 0;
        let msg = vec![FN4, 31i32];
        assert!(!enc_b_fits_row(&mut cws, &mut i, &msg, 2));
        assert_eq!(i, 0);
        assert!(
            cws.is_empty(),
            "enc_b_fits_row(rem=2, FN4+31) returned false → cws must remain empty (no partial mutation); got len={}",
            cws.len()
        );

        // rem=1, FN4: doesn't fit (rem != 2).
        let mut cws: Vec<u32> = Vec::new();
        let mut i: usize = 0;
        let msg = vec![FN4, b'a' as i32];
        assert!(!enc_b_fits_row(&mut cws, &mut i, &msg, 1));
        assert_eq!(i, 0);

        // Kills `188:17 && with ||` (matches the enc_a_fits_row guard).
        // With rem=3 + FN4 head, original takes the ordinary push path;
        // mutant takes the FN4 sub-branch and returns false.
        let mut cws: Vec<u32> = Vec::new();
        let mut i: usize = 0;
        let msg = vec![FN4, b'b' as i32];
        assert!(enc_b_fits_row(&mut cws, &mut i, &msg, 3));
        assert_eq!(i, 1);
        assert_eq!(cws, vec![100]); // FN4 = 100 in set B
    }

    /// Kills `codewords` orchestrator mutants by pinning a broader set
    /// of bwip-js-oracle goldens that exercise specific branches the
    /// 6-input baseline corpus doesn't reach:
    ///
    ///   * Mixed A/B/C transitions on small column counts (rem-boundary
    ///     branches: `remnums % 2 == 0 && rem >= 3` vs `!= 0 && >= 4`).
    ///   * Multi-row digit-only payloads (`Branch 7` digit-pair emission
    ///     across row boundaries, `pad_row` alternation).
    ///   * Mid-payload C→A and C→B switch-outs (`Branch 4` taking each
    ///     sub-branch).
    ///   * Alternating-case payloads (`Branch 2`/`Branch 3` SFT/SWA/SWB
    ///     emission with `bbeforea`/`abeforeb` predicates).
    ///   * Inputs whose total length forces specific `last_row` and
    ///     `pad_row` paths (k1/k2 mapping, row-indicator + 42 arithmetic).
    ///
    /// Each tuple was generated by
    /// `node node-sidecar/oracle-codablockf.js <text> <columns>`.
    #[test]
    fn codewords_match_extended_bwip_js_oracle_corpus() {
        let extended: &[(&str, usize, &[u32])] = &[
            // Digit-heavy 16-char payload at columns=4 → forces multi-row
            // C-subset run plus the C→B switch-out at the last row.
            (
                "1234567890ABCDEF",
                4,
                &[
                    103, 99, 2, 12, 34, 56, 78, 96, 104, 103, 99, 43, 90, 100, 33, 34, 91, 104,
                    103, 100, 12, 35, 36, 37, 38, 65, 104, 103, 100, 13, 99, 100, 46, 62, 86, 104,
                ],
            ),
            // Lowercase→uppercase→digits at columns=5 → exercises B→A and
            // B→C transitions with rem-tight boundary.
            (
                "abcDEFghi123",
                5,
                &[
                    103, 100, 65, 65, 66, 67, 36, 37, 57, 104, 103, 100, 11, 38, 71, 72, 73, 17,
                    98, 104, 103, 99, 44, 23, 100, 99, 15, 84, 78, 104,
                ],
            ),
            // Triple-A run + 3 digits + triple-B at columns=4 → exercises
            // the A→C 3-digit split (odd remnums fallback branch) and
            // C→B switch-out within a 4-column row.
            (
                "AAA111bbb",
                4,
                &[
                    103, 100, 65, 33, 33, 33, 17, 7, 104, 103, 99, 43, 11, 100, 66, 66, 5, 104,
                    103, 100, 12, 66, 99, 37, 3, 97, 104,
                ],
            ),
            // Mixed A↔B↔C alternation at columns=6 → exercises every
            // Branch-1/2/3 transition twice across rows.
            (
                "abcd1234efgh5678",
                6,
                &[
                    103, 100, 65, 65, 66, 67, 68, 17, 18, 47, 104, 103, 99, 43, 34, 100, 69, 70,
                    71, 72, 53, 104, 103, 99, 44, 56, 78, 100, 99, 66, 46, 16, 104,
                ],
            ),
            // Pure 14-char A run at columns=4 → exercises 4 rows of A-only
            // with the `last_row` k1/k2 mapping via abmap (`use_cmap=false`).
            (
                "ABCDEFGHIJKLMN",
                4,
                &[
                    103, 100, 66, 33, 34, 35, 36, 34, 104, 103, 100, 11, 37, 38, 39, 40, 99, 104,
                    103, 100, 12, 41, 42, 43, 44, 70, 104, 103, 100, 13, 45, 46, 59, 90, 44, 104,
                ],
            ),
            // 3-char lowercase at columns=4 → small payload where the
            // last row holds only a SWC pad before the k1/k2 slots.
            (
                "abc",
                4,
                &[
                    103, 100, 64, 65, 66, 67, 99, 71, 104, 103, 100, 11, 99, 100, 52, 6, 85, 104,
                ],
            ),
            // 2-char A payload at columns=4 → pinned exactly: row 1 only
            // contains pad codewords plus the k1/k2 slots.
            (
                "AB",
                4,
                &[
                    103, 100, 64, 33, 34, 99, 100, 13, 104, 103, 100, 11, 99, 100, 89, 44, 86, 104,
                ],
            ),
            // 5-char A payload at columns=4 → exact-fit single-row data
            // then last_row pad.
            (
                "ABCDE",
                4,
                &[
                    103, 100, 64, 33, 34, 35, 36, 30, 104, 103, 100, 11, 37, 99, 69, 78, 0, 104,
                ],
            ),
            // Alternating case 6-char at columns=4 → exercises Branch 3's
            // SFT-into-B path (`abeforeb(i+1)` true) repeatedly.
            (
                "AbCdEf",
                4,
                &[
                    103, 100, 64, 33, 66, 35, 68, 41, 104, 103, 100, 11, 37, 70, 77, 92, 8, 104,
                ],
            ),
            // 5-char B-then-C payload at columns=5 → exercises Branch 3
            // (single 'a' starts in B, then digit run forces row break).
            (
                "a1234",
                5,
                &[
                    103, 100, 64, 65, 99, 12, 34, 100, 32, 104, 103, 100, 11, 99, 100, 99, 35, 80,
                    24, 104,
                ],
            ),
            // Branch 2 SWA + Branch 3 SWB-else: three leading control bytes
            // force SFT row selector (cset=A), then 'abc' lands in
            // Branch 3's inner-else path (push_seta(SWB); cset=B;
            // push_setb(msg[i]); i += 1) on lines 359-362. Without an
            // input that takes the inner-else, the `i += 1` mutants on
            // line 362 are unreachable.
            (
                "\x01\x01\x01abc",
                5,
                &[
                    103, 98, 64, 65, 65, 65, 100, 65, 1, 104, 103, 100, 11, 66, 67, 99, 16, 63, 75,
                    104,
                ],
            ),
            // Branch 2 + Branch 3 SFT-if: alternating control/lowercase
            // forces Branch 3's inner-if path (push_seta(SFT);
            // push_setb(msg[i]); i += 1) on lines 354-357. The
            // `i + 1 < msg.len()` boundary at 354:30 and the
            // `abeforeb(i + 1)` argument at 354:56 only matter when
            // Branch 3's inner-if is reached with a non-empty `i+1`
            // lookahead.
            (
                "\x01a\x01b",
                4,
                &[
                    103, 98, 64, 65, 98, 65, 65, 86, 104, 103, 100, 11, 66, 99, 74, 49, 41, 104,
                ],
            ),
            // Branch 4 fall-through C→A: 4 digits at the start force
            // cset=C, then the trailing control bytes force cset=C → A
            // (push 101=SWA-in-C) and enc_a_fits_row picks up the
            // control byte. Exercises lines 371-376 (the `rem - 1`
            // argument at 374:70 and the SWA branch arithmetic).
            (
                "1234\x01\x01",
                5,
                &[
                    103, 99, 0, 12, 34, 101, 65, 65, 76, 104, 103, 100, 11, 99, 100, 99, 5, 59, 6,
                    104,
                ],
            ),
            // Branch 4 fall-through C→B: same shape as above but lowercase
            // tail forces C → B (push 100 = SWB-in-C). Exercises lines
            // 378-382 (the `rem - 1` argument at 380:70 and the SWB
            // branch arithmetic).
            (
                "1234ab",
                5,
                &[
                    103, 99, 0, 12, 34, 100, 65, 66, 78, 104, 103, 100, 11, 99, 100, 99, 35, 68,
                    43, 104,
                ],
            ),
            // 1-character minimum payload at columns=4: exercises the
            // last_row exact-fit path with k1/k2 specific arithmetic
            // (single 'A' byte → k1 = 33*1 + 33 mod 86 = 66, k2 = 33
            // mod 86 = 33; map(false, _) goes through abmap to slots
            // 43, 64). Kills the last-row `+= with *= / -=` mutants on
            // line 416 because i and r are exactly 1 here.
            (
                "A",
                4,
                &[
                    103, 100, 64, 33, 99, 100, 99, 66, 104, 103, 100, 11, 99, 100, 43, 64, 79, 104,
                ],
            ),
            // 10 lowercase chars at columns=5 → 3 rows; row 2's data
            // slots are pad alternation (99=SWC, 100=swb-in-C, 99) →
            // exercises pad_row's A/B/C alternation at the boundary.
            (
                "abcdefghij",
                5,
                &[
                    103, 100, 65, 65, 66, 67, 68, 69, 61, 104, 103, 100, 11, 70, 71, 72, 73, 74,
                    78, 104, 103, 100, 12, 99, 100, 99, 75, 6, 57, 104,
                ],
            ),
            // 10-digit + 2-lowercase at columns=4: digit pairs split
            // across 2 rows of cset=C, then a final row with cset=C → B
            // switch-out for 'ab'. Combines Branch 7 multi-row digit
            // emission with the Branch 4 final-row switch-out path.
            (
                "1234567890ab",
                4,
                &[
                    103, 99, 1, 12, 34, 56, 78, 94, 104, 103, 99, 43, 90, 100, 65, 66, 31, 104,
                    103, 100, 12, 99, 100, 72, 40, 82, 104,
                ],
            ),
            // `"a12345"` cols=5: forces Branch 1's ODD-remnums fallback
            // (lines 307-330). At i=1 cset=B, remnums=5 (odd), rem=4,
            // so the path `push 'a' in B; SWC; cset=C; 2 digit pairs`
            // fires. Without this input, the odd-remnums fallback is
            // unreachable and the `+= with *= / -=` mutants on lines
            // 314, 324, 326, 327 survive.
            (
                "a12345",
                5,
                &[
                    103, 100, 64, 65, 17, 99, 23, 45, 100, 104, 103, 100, 11, 99, 100, 99, 9, 1,
                    36, 104,
                ],
            ),
            // `"ab12345"` cols=6: same Branch 1 odd-remnums fallback at
            // i=2 (cset=B, remnums=5, rem=4). Pairs with `a12345` to
            // distinguish the `% with /` vs `% with +` mutants on line
            // 307:35 — different rem values exercise the boundary.
            (
                "ab12345",
                6,
                &[
                    103, 100, 64, 65, 66, 17, 99, 23, 45, 33, 104, 103, 100, 11, 99, 100, 99, 100,
                    94, 74, 74, 104,
                ],
            ),
            // `"A12345"` cols=5: same odd-remnums path but the leading
            // 'A' takes the SFT-A row selector (cset starts A for the
            // first byte, then switches back). Pinning this differs
            // from `a12345` by the row-0 subset-selector codeword.
            (
                "A12345",
                5,
                &[
                    103, 100, 64, 33, 17, 99, 23, 45, 4, 104, 103, 100, 11, 99, 100, 99, 63, 1, 51,
                    104,
                ],
            ),
            // `"AaBb"` cols=4: alternating-case 4-char input. Both
            // 'A' and 'a' are in set B (so Branch 6 alone handles
            // them), but the preprocessing arrays `next_anotb` and
            // `next_bnota` produce distinct values at every position
            // — pinning the codeword sequence catches the mutation
            // class where row indicator / k1/k2 arithmetic depends
            // on those arrays even when the active branch doesn't.
            (
                "AaBb",
                4,
                &[
                    103, 100, 64, 33, 65, 34, 66, 20, 104, 103, 100, 11, 99, 100, 53, 71, 68, 104,
                ],
            ),
            // `"a\x01b"` cols=4: Branch 2 SFT-if path (mirror of the
            // Branch 3 SFT-if already covered by `\x01a\x01b`). At
            // i=1 cset=B, msg[i]=\x01 anotb, bbeforea(i+1=2)=true ('b'
            // is bnota) → SFT (lines 338-340) fires. Kills the line 337
            // `&& with ||` boundary mutants and the 340 `i += 1` arm.
            (
                "a\x01b",
                4,
                &[
                    103, 100, 64, 65, 98, 65, 66, 94, 104, 103, 100, 11, 99, 100, 27, 89, 46, 104,
                ],
            ),
            // `"abc\x01\x02defg"` cols=5: long-B prefix then two control
            // bytes then long-B tail. At i=3 cset=B, msg[3]=\x01 anotb,
            // bbeforea(4) — msg[4]=\x02 anotb, msg[5]='d' bnota. So
            // next_bnota[4] is small, next_anotb[4]=0. bbeforea(4)=
            // false → SWA path (lines 342-345) fires. Then in row 1
            // we're cset=A with 'd','e' etc bnota → Branch 3 SWB-else
            // fires again. Exercises the alternate Branch 2 path.
            (
                "abc\x01\x02defg",
                5,
                &[
                    103, 100, 65, 65, 66, 67, 101, 65, 25, 104, 103, 98, 11, 66, 100, 68, 69, 70,
                    5, 104, 103, 100, 12, 71, 99, 100, 10, 91, 76, 104,
                ],
            ),
            // `"abcd12345"` cols=5: rem-tight Branch 1 odd-remnums entry
            // at i=4. After 'a','b','c','d' fill positions 3-6 of row 0,
            // i=4 is at column 7 with rem=8-7=1 — Branch 1 entry fails
            // (rem<4). Falls to next row. In row 1 i=4 with full rem,
            // remnums=5 odd → Branch 1 odd fires with full rem. Pins
            // the row-0 boundary AND the Branch-1-odd path together.
            (
                "abcd12345",
                5,
                &[
                    103, 100, 64, 65, 66, 67, 68, 17, 4, 104, 103, 99, 43, 23, 45, 100, 1, 50, 54,
                    104,
                ],
            ),
            // 10 pure-A control bytes at cols=4: multi-row symbol
            // entirely in subset A (every byte is anotb). Row 0 selector
            // is 98=SFT (cset=A initial), each row's data slots are 4
            // A-encoded control bytes. Exercises pad_row's A→C
            // alternation never (since cset stays A throughout) and
            // pins the last-row r-counter arithmetic on a 3-row symbol.
            (
                "\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a",
                4,
                &[
                    103, 98, 65, 65, 66, 67, 68, 91, 104, 103, 98, 11, 69, 70, 71, 72, 55, 104,
                    103, 98, 12, 73, 74, 9, 50, 55, 104,
                ],
            ),
        ];
        for &(text, c, expected) in extended {
            let got = codewords(&to_msg(text), c)
                .unwrap_or_else(|e| panic!("codewords({text:?}, {c}) failed: {e}"));
            assert_eq!(
                got.as_slice(),
                expected,
                "codewords({text:?}, {c}) mismatch:\n  got:    {got:?}\n  wanted: {expected:?}",
            );
        }
    }

    /// Kills the Branch 1 FN1-pair mutants on lines 300, 324
    /// (`i += 1` inside `for _ in 0..2 { if msg[i] == FN1 { ... i += 1; } }`).
    /// FN1 is the i32 sentinel `-5` injected by GS1 preprocessing
    /// upstream of `codewords`; it cannot be produced via the public
    /// `encode(text, opts)` API (which only converts ASCII bytes).
    /// This test calls `codewords(&[i32], usize)` directly with FN1
    /// inline at various positions inside a digit run, pinning the
    /// codeword sequence bwip-js's parsefnc-enabled oracle produces
    /// for the equivalent `^FNC1` escape input.
    #[test]
    fn codewords_handles_fn1_inside_digit_pair() {
        // bwip-js parsefnc:true substitutes `^FNC1` → FN1 sentinel
        // mid-stream. Goldens captured via
        // `node oracle-codablockf.js "<input>" <cols>` with parsefnc:true
        // toggled in the oracle script.
        let cases: &[(&str, &[i32], usize, &[u32])] = &[
            // "12^FNC134" cols=5 → msg = ['1','2', FN1, '3','4'].
            // Branch 1 entry at i=0: remnums = numsscr counts FN1 with
            // s%2==0 half-pair → (n=4, s=5). remnums = min(5, 2*5)=5.
            // odd-remnums branch (rem=5>=4) fires. Pushes '1' in B…
            // actually wait — the row selector picks SWC first because
            // nums=5>=2. So we go directly to subset C. Inside subset C
            // (Branch 7), FN1 emits push_c_fn1=102.
            (
                "12^FNC134",
                &[b'1' as i32, b'2' as i32, FN1, b'3' as i32, b'4' as i32],
                5,
                &[
                    103, 99, 0, 12, 102, 34, 100, 99, 49, 104, 103, 100, 11, 99, 100, 99, 12, 39,
                    11, 104,
                ],
            ),
            // "1234^FNC156" cols=5 → msg = ['1','2','3','4', FN1, '5','6'].
            // FN1 appears AFTER the first 4 digit chars (i=4, s%2==0).
            // numsscr at i=0 returns (n=6, s=7). Branch 1 odd-remnums
            // fires only if remnums is odd → 7 is odd, rem must be >=4.
            (
                "1234^FNC156",
                &[
                    b'1' as i32,
                    b'2' as i32,
                    b'3' as i32,
                    b'4' as i32,
                    FN1,
                    b'5' as i32,
                    b'6' as i32,
                ],
                5,
                &[
                    103, 99, 0, 12, 34, 102, 56, 100, 66, 104, 103, 100, 11, 99, 100, 99, 61, 67,
                    89, 104,
                ],
            ),
            // "ab^FNC1cd" cols=5 → msg = ['a','b', FN1, 'c','d'].
            // 'a','b' are bnota, FN1 is in_set_c (so numsscr at i=2 sees
            // FN1 as a digit-slot). But at i=0 numsscr returns 0 (a is
            // not in C). So row starts in B (default selector). Branch 6
            // emits 'a','b'. Then at i=2 FN1, cset=B, Branch 1 entry
            // (remnums >= 4?) — depends on rem.
            (
                "ab^FNC1cd",
                &[b'a' as i32, b'b' as i32, FN1, b'c' as i32, b'd' as i32],
                5,
                &[
                    103, 100, 64, 65, 66, 102, 67, 68, 15, 104, 103, 100, 11, 99, 100, 99, 72, 79,
                    33, 104,
                ],
            ),
            // "a12^FNC134" cols=6 → msg = ['a','1','2', FN1, '3','4'].
            // At i=0 'a' (not in C) → row selector SWB cset=B. Push 'a' in
            // B. At i=1, numsscr returns (5, 6) with FN1 occupying an
            // even-s half-pair slot. remnums=6 (even) → Branch 1 first
            // arm fires. Inside the 2-iter for loop: iter 1 is
            // push_c_pair('1','2'), iter 2 is `msg[i]==FN1` → push_c_fn1
            // + `i += 1`. **Kills the 300:31 `i += 1` mutants** (FN1
            // path in Branch 1 even-remnums).
            (
                "a12^FNC134",
                &[
                    b'a' as i32,
                    b'1' as i32,
                    b'2' as i32,
                    FN1,
                    b'3' as i32,
                    b'4' as i32,
                ],
                6,
                &[
                    103, 100, 64, 65, 99, 12, 102, 34, 100, 57, 104, 103, 100, 11, 99, 100, 99,
                    100, 60, 76, 58, 104,
                ],
            ),
            // "a1234^FNC156" cols=7 → msg = ['a','1','2','3','4', FN1,
            // '5','6']. After 'a' in B, Branch 1 entry at i=1: numsscr
            // returns (7, 8). With remnums=8 even and rem=6, Branch 1
            // first-arm 2-iter loop: pair('1','2'), pair('3','4'), then
            // SWC+stuff. Wait actually 2 iters consume 4 chars; FN1 at
            // position 5 isn't reached. Let me check: iter1
            // push_c_pair(msg[1],msg[2])=12, i=3; iter2
            // push_c_pair(msg[3],msg[4])=34, i=5. After Branch 1, the
            // row continues at i=5 (FN1) with cset=C. Branch 7 fires
            // FN1 in C. Pins a longer Branch-1-even-into-Branch-7-FN1
            // transition.
            (
                "a1234^FNC156",
                &[
                    b'a' as i32,
                    b'1' as i32,
                    b'2' as i32,
                    b'3' as i32,
                    b'4' as i32,
                    FN1,
                    b'5' as i32,
                    b'6' as i32,
                ],
                7,
                &[
                    103, 100, 64, 65, 99, 12, 34, 102, 56, 100, 55, 104, 103, 100, 11, 99, 100, 99,
                    100, 99, 66, 61, 79, 104,
                ],
            ),
            // "ab12^FNC134" cols=6 → msg = ['a','b','1','2', FN1, '3','4'].
            // Two-byte B prefix then Branch 1 entry at i=2. numsscr at i=2
            // returns (5, 6). remnums=6 even. Iter 1 push_c_pair('1','2'),
            // iter 2 push_c_fn1. Same FN1-in-Branch-1 path but starting
            // at i=2, distinguishing from the i=1 cases above.
            (
                "ab12^FNC134",
                &[
                    b'a' as i32,
                    b'b' as i32,
                    b'1' as i32,
                    b'2' as i32,
                    FN1,
                    b'3' as i32,
                    b'4' as i32,
                ],
                6,
                &[
                    103, 100, 64, 65, 66, 99, 12, 102, 34, 77, 104, 103, 100, 11, 99, 100, 99, 100,
                    57, 61, 20, 104,
                ],
            ),
        ];
        for &(label, msg, c, expected) in cases {
            let got = codewords(msg, c)
                .unwrap_or_else(|e| panic!("codewords FN1 case {label:?}, cols={c} failed: {e}"));
            assert_eq!(
                got.as_slice(),
                expected,
                "codewords FN1 case {label:?}, cols={c} mismatch:\n  got:    {got:?}\n  wanted: {expected:?}",
            );
        }
    }

    /// Compare rendered bar geometry byte-for-byte against bwip-js's
    /// `toSVG` path output. Goldens captured via
    /// `node-sidecar/verify-codablockf.js "AB" 8`.
    #[test]
    fn rendered_bars_match_bwip_js() {
        // (text, columns, expected_rows): each row is the alternating
        // bar-space run-length sequence starting with a bar.
        let cases: &[(&str, usize, &[&[u8]])] = &[(
            "AB",
            8,
            &[
                &[
                    2, 1, 1, 4, 1, 2, 1, 1, 4, 1, 3, 1, 1, 1, 1, 4, 2, 2, 1, 1, 1, 3, 2, 3, 1, 3,
                    1, 1, 2, 3, 1, 1, 3, 1, 4, 1, 1, 1, 4, 1, 3, 1, 1, 1, 3, 1, 4, 1, 1, 1, 4, 1,
                    3, 1, 1, 1, 3, 1, 4, 1, 1, 1, 4, 1, 3, 1, 3, 1, 1, 1, 4, 1, 2, 3, 3, 1, 1, 1,
                    2,
                ],
                &[
                    2, 1, 1, 4, 1, 2, 1, 1, 4, 1, 3, 1, 2, 3, 1, 2, 1, 2, 1, 1, 3, 1, 4, 1, 1, 1,
                    4, 1, 3, 1, 1, 1, 3, 1, 4, 1, 1, 1, 4, 1, 3, 1, 1, 1, 3, 1, 4, 1, 1, 1, 4, 1,
                    3, 1, 2, 1, 2, 1, 4, 1, 1, 3, 2, 1, 3, 1, 1, 2, 2, 1, 3, 2, 2, 3, 3, 1, 1, 1,
                    2,
                ],
            ],
        )];
        for &(text, c, expected_rows) in cases {
            let mut opts = Options::default();
            opts.extras.push(("columns".into(), c.to_string()));
            // Stage 11.A8c — input + column-count echo (parallel to
            // line 695 site) so a regression points at the specific
            // corpus row that stopped encoding.
            let stacked = encode(text, &opts).unwrap_or_else(|e| {
                panic!("codablockf encode(text={text:?}, columns={c}) failed: {e:?}");
            });
            assert_eq!(
                stacked.rows.len(),
                expected_rows.len(),
                "{text:?} row count"
            );
            for (i, (row, want)) in stacked.rows.iter().zip(expected_rows).enumerate() {
                assert_eq!(
                    row.bars.as_slice(),
                    *want,
                    "{text:?} row {i}: bar runs mismatch"
                );
            }
        }
    }

    /// Stage 11.A8c — pin `push_c_pair(cws, a, b)`. Packs two ASCII
    /// digit chars `a` and `b` into a single Code 128 numeric codeword
    /// `(a - 48) * 10 + (b - 48)`. Pins the canonical decoding so the
    /// classic arithmetic mutations are killed:
    ///   * `* 10` → `+ 10` (would map ('1','0') → 1+0 = 1, not 10);
    ///   * `* 10` → `* 100` (would map ('1','0') → 100, not 10);
    ///   * `- 48` → `+ 48` on either operand (shifts everything by 48);
    ///   * argument swap (`(b - 48) * 10 + (a - 48)`) — caught by the
    ///     asymmetric ('1','0') vs ('0','1') anchors;
    ///   * `replace push_c_pair with ()` (no codeword pushed).
    #[test]
    fn push_c_pair_packs_two_ascii_digits_into_one_codeword() {
        // ('0', '0') → 0
        let mut cws: Vec<u32> = Vec::new();
        push_c_pair(&mut cws, b'0' as i32, b'0' as i32);
        assert_eq!(cws, vec![0]);

        // ('1', '0') → 10 (kills `* 10` → `+ 10` / `* 100`).
        let mut cws = vec![];
        push_c_pair(&mut cws, b'1' as i32, b'0' as i32);
        assert_eq!(cws, vec![10]);

        // ('0', '1') → 1 (asymmetric anchor; kills argument swap).
        let mut cws = vec![];
        push_c_pair(&mut cws, b'0' as i32, b'1' as i32);
        assert_eq!(cws, vec![1]);

        // ('5', '3') → 53 (mid-range anchor).
        let mut cws = vec![];
        push_c_pair(&mut cws, b'5' as i32, b'3' as i32);
        assert_eq!(cws, vec![53]);

        // ('9', '9') → 99 (top boundary).
        let mut cws = vec![];
        push_c_pair(&mut cws, b'9' as i32, b'9' as i32);
        assert_eq!(cws, vec![99]);

        // Pushes (not replaces) — preserves prior buffer contents.
        let mut cws = vec![777];
        push_c_pair(&mut cws, b'2' as i32, b'7' as i32);
        assert_eq!(cws, vec![777, 27]);
    }

    /// Stage 11.A8c — pin `pad_row(cws, cset, n)`. A 3-arm subset
    /// state machine that emits `n` subset-switch codewords to flush a
    /// row. Per-arm behavior:
    ///   * A → push SWC (=99) and transition to C;
    ///   * B → push SWC (=99) and transition to C;
    ///   * C → push 100 (swb literal) and transition to B.
    ///
    /// Anchors pin:
    ///   * n=0 → no codewords pushed, state unchanged;
    ///   * single step from each starting state writes the right
    ///     codeword AND transitions to the right next state;
    ///   * a multi-step run from C demonstrates the C↔B oscillation
    ///     (100, 99, 100, 99, ...);
    ///   * A→C and B→C use the same codeword (99) but C→B uses a
    ///     different one (100), so swapping the two would diverge.
    ///   * push-not-replace: pre-existing buffer entries preserved.
    ///
    /// Kills `replace pad_row with ()`, per-arm body-swap mutants, and
    /// the swap-of-arms mutants on the next-state assignment.
    #[test]
    fn pad_row_state_machine_walks_oscillating_swc_then_swb() {
        // n=0: no-op even if state is set; state stays put.
        let mut cws = vec![42];
        let mut cset = Subset::A;
        pad_row(&mut cws, &mut cset, 0);
        assert_eq!(cws, vec![42], "n=0 must not push");
        assert_eq!(cset, Subset::A, "n=0 must not transition");

        // n=1 from A: push 99 (SWC), state → C.
        let mut cws = vec![];
        let mut cset = Subset::A;
        pad_row(&mut cws, &mut cset, 1);
        assert_eq!(cws, vec![99], "A → SWC=99");
        assert_eq!(cset, Subset::C, "A → C");

        // n=1 from B: same codeword (99) but starting state different.
        let mut cws = vec![];
        let mut cset = Subset::B;
        pad_row(&mut cws, &mut cset, 1);
        assert_eq!(cws, vec![99], "B → SWC=99");
        assert_eq!(cset, Subset::C, "B → C");

        // n=1 from C: different codeword (100), state → B.
        let mut cws = vec![];
        let mut cset = Subset::C;
        pad_row(&mut cws, &mut cset, 1);
        assert_eq!(
            cws,
            vec![100],
            "C → swb=100 (NOT 99 — kills swc/swb arm swap)"
        );
        assert_eq!(cset, Subset::B, "C → B");

        // n=3 from A: A→C (99), C→B (100), B→C (99).
        let mut cws = vec![];
        let mut cset = Subset::A;
        pad_row(&mut cws, &mut cset, 3);
        assert_eq!(cws, vec![99, 100, 99], "A → C → B → C");
        assert_eq!(cset, Subset::C);

        // n=4 from C: C→B (100), B→C (99), C→B (100), B→C (99).
        let mut cws = vec![];
        let mut cset = Subset::C;
        pad_row(&mut cws, &mut cset, 4);
        assert_eq!(cws, vec![100, 99, 100, 99], "C↔B oscillation");
        assert_eq!(cset, Subset::C);

        // Push-not-replace: pre-existing entries preserved.
        let mut cws = vec![555, 666];
        let mut cset = Subset::A;
        pad_row(&mut cws, &mut cset, 2);
        assert_eq!(cws, vec![555, 666, 99, 100]);
    }

    /// Stage 11.A8c — pin `push_c_fn1(cws)`. Pushes the Code 128
    /// numeric-mode FNC1 codeword (= 102) to the buffer. Used by the
    /// Codablock-F GS1 chain at the start of every CC-A/CC-B + FNC1
    /// sequence run. The helper has just one observable property: it
    /// appends exactly the value 102.
    ///
    /// Mutations killed:
    ///   * `cws.push(102)` → `cws.push(101)` / `cws.push(103)` (codeword
    ///     drift — would break Code 128 spec compliance);
    ///   * `cws.push(102)` → `cws.push(0)` / other constant swaps;
    ///   * `cws.push(102)` → no-op (function removed by mutant);
    ///   * `cws.push(102); cws.push(102)` (double-push);
    ///   * `cws[0] = 102` (replace mode mutant — would clobber pre-
    ///     existing buffer contents instead of appending).
    #[test]
    fn push_c_fn1_appends_exactly_codeword_102() {
        // Empty buffer → [102].
        let mut cws: Vec<u32> = Vec::new();
        push_c_fn1(&mut cws);
        assert_eq!(cws, vec![102], "FN1 codeword in Code 128 = 102");

        // Multiple calls accumulate.
        push_c_fn1(&mut cws);
        push_c_fn1(&mut cws);
        assert_eq!(cws, vec![102, 102, 102], "3 calls produce 3 102s");

        // Push-not-replace: pre-existing entries preserved.
        let mut cws: Vec<u32> = vec![1, 2, 3];
        push_c_fn1(&mut cws);
        assert_eq!(cws, vec![1, 2, 3, 102], "pushes after existing entries");
        // Single push appends exactly one entry.
        assert_eq!(cws.len(), 4, "single push adds exactly 1 entry");

        // Distinct from neighbouring Code 128 codewords (asymmetric
        // anchor — kills off-by-one mutants on the constant value).
        let mut cws: Vec<u32> = Vec::new();
        push_c_fn1(&mut cws);
        assert_ne!(cws[0], 100, "must not be 100 (SWB in set C)");
        assert_ne!(cws[0], 101, "must not be 101 (SWA)");
        assert_ne!(cws[0], 103, "must not be 103 (Start A)");
    }

    /// Stage 11.A8c — pin `push_seta(cws, v)`. Thin wrapper around
    /// `seta(v)`: delegates the per-token lookup, unwraps via
    /// `.expect("internal: enca on non-set-A value")`, and appends
    /// the codeword to `cws`. Used by every Codablock-F encoder path
    /// that's confirmed the token is set-A-encodable.
    ///
    /// Anchors pin the delegation chain + push semantics:
    ///   * push_seta(0) → 64 (control range 0..=31 maps to tok + 64);
    ///   * push_seta(31) → 95 (top of control range);
    ///   * push_seta(32) → 0 (printable range 32..=95 maps to tok - 32);
    ///   * push_seta(b'A' as i32) → 33;
    ///   * push_seta(FN1) → 102 (FN1 sentinel codeword);
    ///   * delegation: push_seta(b'A') result matches seta(b'A').unwrap();
    ///   * push-not-replace: pre-existing buffer entries preserved;
    ///   * push exactly one entry per call.
    ///
    /// Kills `cws.push(seta(v).expect(...))` → `cws.push(setb(v)...)`
    /// (the A↔B swap mutant — A and B disagree on byte 0 (set A: 64,
    /// set B: rejected) and on FN4 mapping (A: 101, B: 100)).
    #[test]
    fn push_seta_delegates_to_seta_and_appends() {
        // Empty buffer: control range 0 → 64.
        let mut cws: Vec<u32> = Vec::new();
        push_seta(&mut cws, 0);
        assert_eq!(cws, vec![64], "seta(0) = tok + 64 = 64");

        // Control top: 31 → 95.
        let mut cws = Vec::new();
        push_seta(&mut cws, 31);
        assert_eq!(cws, vec![95]);

        // Printable start: 32 → 0.
        let mut cws = Vec::new();
        push_seta(&mut cws, 32);
        assert_eq!(cws, vec![0]);

        // 'A' → 33.
        let mut cws = Vec::new();
        push_seta(&mut cws, b'A' as i32);
        assert_eq!(cws, vec![33]);

        // FN1 sentinel → 102.
        let mut cws = Vec::new();
        push_seta(&mut cws, FN1);
        assert_eq!(cws, vec![102]);

        // Delegation: result matches seta() directly.
        let mut cws = Vec::new();
        push_seta(&mut cws, b'A' as i32);
        assert_eq!(
            cws[0],
            seta(b'A' as i32).unwrap(),
            "push_seta must match seta() directly"
        );

        // Push-not-replace.
        let mut cws: Vec<u32> = vec![999, 888];
        push_seta(&mut cws, b'A' as i32);
        assert_eq!(cws, vec![999, 888, 33]);

        // A↔B asymmetry: push_seta(0) = 64, but setb(0) = None
        // (printable range starts at 32). So if the A path were
        // swapped to use setb, push_seta(0) would panic. Pinning
        // push_seta(0) → 64 (Ok) implicitly kills that swap.
        assert!(setb(0).is_none(), "precondition: setb(0) is None");
    }

    /// Stage 11.A8c — pin `push_setb(cws, v)`. Sister of push_seta:
    /// delegates to `setb(v)` and appends the codeword. Used by every
    /// Codablock-F encoder path that's already confirmed the token is
    /// set-B-encodable.
    ///
    /// Anchors pin the delegation chain + push semantics:
    ///   * push_setb(32) → 0 (printable range 32..=127 maps to tok-32);
    ///   * push_setb(b'A') → 33;
    ///   * push_setb(b'a') → 65 (B-only — A rejects lowercase);
    ///   * push_setb(127) → 95 (top of printable range);
    ///   * push_setb(FN4) → 100 (DIFFERENT from FN4_FROM_A=101 —
    ///     pins B's distinct FN4 codeword);
    ///   * push_setb(FN1) → 102 (shared with set A);
    ///   * delegation: push_setb(b'a') == setb(b'a').unwrap();
    ///   * push-not-replace.
    ///
    /// Strong B↔A swap-kill via the 'a' (97) anchor: seta(97) = None
    /// (lowercase not in set A), so any `setb(v)` → `seta(v)` mutant
    /// would panic on push_setb(b'a').
    #[test]
    fn push_setb_delegates_to_setb_and_appends() {
        // Printable range: 32 → 0, 'A' → 33, 127 → 95.
        let mut cws: Vec<u32> = Vec::new();
        push_setb(&mut cws, 32);
        assert_eq!(cws, vec![0], "setb(32) = tok - 32 = 0");

        let mut cws = Vec::new();
        push_setb(&mut cws, b'A' as i32);
        assert_eq!(cws, vec![33]);

        let mut cws = Vec::new();
        push_setb(&mut cws, 127);
        assert_eq!(cws, vec![95], "setb(127) = top of B range");

        // Lowercase: 'a' = 97 → 65 (B-only).
        let mut cws = Vec::new();
        push_setb(&mut cws, b'a' as i32);
        assert_eq!(cws, vec![65], "setb('a') = 97-32 = 65");

        // Sentinel: FN4 → 100 (asymmetric vs FN4_FROM_A=101).
        let mut cws = Vec::new();
        push_setb(&mut cws, FN4);
        assert_eq!(cws, vec![100], "setb(FN4) = 100 (NOT 101 — that's A's FN4)");

        // FN1 → 102 (same as set A — shared sentinel).
        let mut cws = Vec::new();
        push_setb(&mut cws, FN1);
        assert_eq!(cws, vec![102]);

        // Delegation: push_setb(b'a') == setb(b'a').unwrap().
        let mut cws = Vec::new();
        push_setb(&mut cws, b'a' as i32);
        assert_eq!(
            cws[0],
            setb(b'a' as i32).unwrap(),
            "push_setb must match setb() directly"
        );

        // Push-not-replace.
        let mut cws: Vec<u32> = vec![111, 222];
        push_setb(&mut cws, b'A' as i32);
        assert_eq!(cws, vec![111, 222, 33]);

        // B↔A swap-kill anchor: seta('a') is None (lowercase rejected),
        // so if push_setb were rewritten to use seta, push_setb(b'a')
        // would panic. Pinning push_setb(b'a') → 65 (Ok) kills that.
        assert!(
            seta(b'a' as i32).is_none(),
            "precondition: seta('a') is None"
        );
    }

    /// Stage 11.A8c — pin `codewords_to_stacked(cws, columns, text)`.
    /// Final renderer step: chunks codewords into rows of size
    /// `columns + 5` and expands each codeword via ENCS into run-length
    /// bars. The stop codeword (index 104) emits 7 bars; all others
    /// emit 6 → each row has `6 * (row_width - 1) + 7 = 6 * row_width + 1`
    /// bars when the last cw is the stop.
    ///
    /// Mutations killed:
    ///   * `columns + 5` → `+ 4` / `+ 6` (would break row chunking);
    ///   * `ENCS[cw as usize]` indexing drift;
    ///   * `ch.to_digit(10)` → other base (would mis-parse bar widths);
    ///   * row text leakage (per-row text must be None);
    ///   * top-level text propagation: Some("…") passes through, None
    ///     stays None.
    #[test]
    fn codewords_to_stacked_chunks_into_rows_and_expands_via_encs() {
        // Build a 1-row symbol with columns=4 (row_width=9). Use [0..=7]
        // for the data cws and 104 (stop) as the final cw. Each non-stop
        // cw emits 6 bars; the stop emits 7 → row bars.len() = 8*6 + 7 = 55.
        let cws: Vec<u32> = vec![0, 1, 2, 3, 4, 5, 6, 7, 104];
        let stacked = codewords_to_stacked(&cws, 4, Some("hi".into()));
        assert_eq!(stacked.rows.len(), 1, "9 cws / row_width=9 → 1 row");
        assert_eq!(stacked.rows[0].bars.len(), 55, "1 stop + 8 non-stop");
        assert_eq!(
            stacked.text.as_deref(),
            Some("hi"),
            "top-level text propagated"
        );
        // Per-row text must be None (only StackedPattern carries text).
        assert!(stacked.rows[0].text.is_none());

        // First 6 bars come from ENCS[0] = "212222" → [2, 1, 2, 2, 2, 2].
        assert_eq!(
            &stacked.rows[0].bars[..6],
            &[2u8, 1, 2, 2, 2, 2],
            "ENCS[0] expansion"
        );
        // Last 7 bars come from ENCS[104] = "2331112" → [2, 3, 3, 1, 1, 1, 2].
        assert_eq!(
            &stacked.rows[0].bars[55 - 7..],
            &[2u8, 3, 3, 1, 1, 1, 2],
            "ENCS[104] stop expansion"
        );

        // 2-row symbol: 18 cws of width 9 → 2 rows.
        let cws: Vec<u32> = vec![
            0, 0, 0, 0, 0, 0, 0, 0, 104, // row 1 with stop
            1, 1, 1, 1, 1, 1, 1, 1, 104, // row 2 with stop
        ];
        let stacked = codewords_to_stacked(&cws, 4, None);
        assert_eq!(stacked.rows.len(), 2);
        assert_eq!(stacked.text, None, "None propagates to StackedPattern");
        // Both rows: 55 bars each.
        for r in &stacked.rows {
            assert_eq!(r.bars.len(), 55);
        }
        // Row 1 first 6 bars: ENCS[0].
        assert_eq!(&stacked.rows[0].bars[..6], &[2, 1, 2, 2, 2, 2]);
        // Row 2 first 6 bars: ENCS[1] = "222122" → [2, 2, 2, 1, 2, 2].
        assert_eq!(&stacked.rows[1].bars[..6], &[2, 2, 2, 1, 2, 2]);

        // Empty cws → 0 rows.
        let stacked = codewords_to_stacked(&[], 4, None);
        assert_eq!(stacked.rows.len(), 0);
    }

    /// Stage 11.A8c — pin five tiny predicate helpers in codablockf
    /// (`in_set_a`, `in_set_b`, `in_set_c`, `anotb`, `bnota`) that drive
    /// the encoder's subset-selection state machine. None of them has a
    /// direct test — they're only exercised through full encoder paths
    /// where a swap mutation can be masked by downstream state.
    ///
    /// Subset A: codepoints 0..=95 + the FN3/FN2/SFT/SWC/SWB/FN4/FN1/STA/STP
    /// sentinels. Subset B: codepoints 32..=127 + most of the same
    /// sentinels (note SWA replaces SWB). Subset C: digits '0'..='9' +
    /// FN1 + SWB/SWA/STA/STP only.
    ///
    /// Anchors below distinguish:
    ///   * 0 / 31 — set A only (lowercase control).
    ///   * 32 / 95 — both A and B (printable ASCII overlap).
    ///   * 96 / 127 — set B only (lowercase + DEL not in A).
    ///   * '0' / '9' — all three sets (digits).
    ///   * '/' (47) / ':' (58) — neither in C (just outside the digit
    ///     range; pins the `'0'..='9'` boundary).
    ///   * 'A' — A and B (uppercase) but NOT C.
    ///   * FN1 — all three sets.
    ///   * FN2 — A and B only (matches the FN2 sentinel arm in seta/setb
    ///     but no sentinel arm in `in_set_c`).
    ///   * SWA / SWB — distinguish the two subset-switch sentinels:
    ///     SWA is only in B + C; SWB is only in A + C.
    ///   * STA / STP — all three sets (universal sentinels).
    ///
    /// Mutations to catch:
    ///   * `seta(tok).is_some()` → `seta(tok).is_none()` (predicate
    ///     negation). Caught by every `true` anchor.
    ///   * `in_set_c`'s `(b'0'..=b'9').contains(&tok)` → `!contains` or
    ///     range shifted to `(b'1'..=b'9')`: caught by '0' / ':' anchors.
    ///   * `in_set_c`'s `matches!(tok, FN1 | SWB | SWA | STA | STP)`
    ///     arm-drop: caught by FN1 / SWA / SWB / STA / STP anchors.
    ///   * `anotb`'s `&&` swapped to `||`: would let bytes that are ONLY
    ///     in B (e.g. 96, 127) report true. Caught by anotb(96)=false.
    ///   * `anotb`'s `!in_set_b` swapped to `in_set_b`: would flip the
    ///     truth value for 0 (in A, not in B). Caught directly.
    ///   * `bnota`'s `&&` ↔ `||` swap: similar discriminators with
    ///     'a'/127/0/31 anchors.
    ///   * `anotb` / `bnota` body swap: caught because anotb('a' = 97)
    ///     = false but bnota('a') = true.
    #[test]
    fn subset_membership_predicates_distinguish_each_set() {
        // ---- in_set_a ----
        // Lowercase control codes: only in A.
        assert!(in_set_a(0), "NUL (0) is in set A (codepoint 0..=31)");
        assert!(in_set_a(31), "31 (US) is in set A");
        // Printable overlap with B.
        assert!(in_set_a(32), "space (32) is in set A (codepoint 32..=95)");
        assert!(in_set_a(95), "underscore (95) is in set A");
        // NOT in A.
        assert!(!in_set_a(96), "backtick (96) is NOT in set A");
        assert!(!in_set_a(127), "DEL (127) is NOT in set A");
        // Digits.
        assert!(in_set_a(b'0' as i32));
        assert!(in_set_a(b'9' as i32));
        // Sentinels.
        assert!(in_set_a(FN1));
        assert!(in_set_a(FN2));
        assert!(in_set_a(FN3));
        assert!(in_set_a(SWB), "SWB is in set A (switch-to-B from A)");
        assert!(!in_set_a(SWA), "SWA is NOT in set A (only in B)");

        // ---- in_set_b ----
        // Lowercase control codes: NOT in B.
        assert!(!in_set_b(0), "NUL (0) is NOT in set B (B starts at 32)");
        assert!(!in_set_b(31), "31 is NOT in set B");
        // Printable.
        assert!(in_set_b(32), "space (32) is in set B");
        assert!(in_set_b(95));
        assert!(in_set_b(96), "backtick (96) is in set B");
        assert!(in_set_b(127), "DEL (127) is in set B");
        // Digits.
        assert!(in_set_b(b'0' as i32));
        // Sentinels.
        assert!(in_set_b(FN1));
        assert!(in_set_b(FN2));
        assert!(in_set_b(SWA), "SWA is in set B (switch-to-A from B)");
        assert!(!in_set_b(SWB), "SWB is NOT in set B");

        // ---- in_set_c ----
        // Digits.
        assert!(in_set_c(b'0' as i32));
        assert!(in_set_c(b'9' as i32));
        // Just below '0' (47 = '/').
        assert!(!in_set_c(b'/' as i32), "'/' (47) is NOT in set C");
        // Just above '9' (58 = ':').
        assert!(!in_set_c(b':' as i32), "':' (58) is NOT in set C");
        // Letters.
        assert!(!in_set_c(b'A' as i32));
        assert!(!in_set_c(b'a' as i32));
        // Sentinels: FN1 + SWA/SWB/STA/STP are in C; FN2/FN3/FN4/SFT/SWC
        // are NOT.
        assert!(in_set_c(FN1));
        assert!(in_set_c(SWA));
        assert!(in_set_c(SWB));
        assert!(in_set_c(STA));
        assert!(in_set_c(STP));
        assert!(!in_set_c(FN2));
        assert!(!in_set_c(FN3));
        assert!(!in_set_c(SFT));
        assert!(!in_set_c(SWC));

        // ---- anotb (in A and NOT in B) ----
        // Lowercase control: in A only.
        assert!(anotb(0), "NUL: in A, not in B → anotb=true");
        assert!(anotb(31), "31: in A, not in B");
        // Overlap.
        assert!(!anotb(32), "space: in both → anotb=false");
        assert!(!anotb(95));
        // B-only.
        assert!(!anotb(96), "backtick: in B not A → anotb=false");
        assert!(!anotb(127), "DEL: in B not A → anotb=false");
        // SWB: in A only (not in B).
        assert!(anotb(SWB), "SWB in A only → anotb=true");

        // ---- bnota (in B and NOT in A) ----
        // B-only.
        assert!(bnota(96), "backtick: in B not A → bnota=true");
        assert!(bnota(b'a' as i32), "'a' (97): in B not A");
        assert!(bnota(127), "DEL: in B not A");
        // Overlap.
        assert!(!bnota(32), "space: in both → bnota=false");
        assert!(!bnota(95));
        // A-only.
        assert!(!bnota(0));
        assert!(!bnota(31));
        // SWA: in B only.
        assert!(bnota(SWA), "SWA in B only → bnota=true");

        // ---- Symmetry: a byte cannot be both anotb and bnota ----
        for tok in [0, 31, 32, 95, 96, 97, 127, b'0' as i32, b'A' as i32] {
            assert!(
                !(anotb(tok) && bnota(tok)),
                "tok={tok}: anotb and bnota must be mutually exclusive"
            );
        }
    }
}
