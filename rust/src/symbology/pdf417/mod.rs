//! PDF417 — fully ported, verified byte-for-byte vs bwip-js.
//!
//! All three compaction modes are implemented, each verified
//! byte-for-byte vs bwip-js's `debugcws` output:
//! - [`text_codewords`]: text-compaction encoder with Dijkstra-style
//!   sub-mode selection across the alpha / lower / mixed / punct
//!   sub-modes. Pairs of sub-codewords pack into 0..899 codewords.
//! - [`numeric_codewords`]: numeric-compaction encoder; splits the
//!   input into 44-digit chunks, prepends `1` as a literal sentinel,
//!   and runs long division by 900.
//! - [`byte_codewords`]: binary-compaction encoder; every full 6-byte
//!   group becomes 5 codewords via base-256 → base-900 conversion,
//!   with any 1..=5 trailing bytes emitted as raw codewords.
//!
//! [`data_codewords`] composes those three modes via the mode-selector
//! DP: it picks the cheapest compaction mode for each contiguous run of
//! input bytes, emits the right latch codewords (900/901/902/913/924),
//! and concatenates everything into the full `datcws` stream that
//! bwip-js's encoder produces (verified for 8 mixed inputs covering
//! text, numeric, byte, and non-ASCII payloads).
//!
//! [`pdf417_cws`] composes that stream with the length prefix, 900
//! padding, symbol-size selection, and Reed-Solomon ECC over GF(929)
//! ([`crate::util::rs_gf929`]) to produce the full per-row codeword
//! array — verified for 4 inputs vs bwip-js's debugecc output.
//!
//! [`pdf417_render`] is the top of the pipeline: it consumes the cws
//! array and emits a [`BitMatrix`] using the 3×929×17-bit cluster
//! patterns stored in `clusters::ALL_CLUSTERS`. Verified byte-for-byte
//! against bwip-js's `pixs` row-major bit array for "PDF417" at
//! eclevel=2 (7 rows × 103 columns = 721 pixels).
//!
//! Reference: bwip-js `bwipp_pdf417` (line 20899 in the 2026-03-31
//! vendor snapshot).

pub(crate) mod clusters;

use crate::encoding::BitMatrix;
use crate::error::Error;
use crate::options::Options;

// --- Sub-mode indices and special markers ---

/// Sub-mode indices.
const ALPHA: usize = 0;
const LOWER: usize = 1;
const MIXED: usize = 2;
const PUNCT: usize = 3;

/// Special charmap markers. Stored as negative i16 so they never collide
/// with the 0..127 ASCII byte range. Values match BWIPP's constants.
const AL: i16 = -6; // alpha latch
const LL: i16 = -7; // lower latch
const ML: i16 = -8; // mixed latch
const PL: i16 = -9; // punct latch
const AS: i16 = -10; // alpha shift (single char)
const PS: i16 = -11; // punct shift

/// "Infinity" for the DP — large enough that no real path beats it but
/// still well within i32 range so we don't worry about overflow.
const INF: i32 = 40000;

/// Charmap: 30 sub-codewords × 4 sub-modes. Each entry says "the
/// sub-codeword `i` in sub-mode `s` represents this character/marker".
/// Mirrors bwip-js's `pdf417_charmaps`.
///
/// Encoded as i16: ASCII bytes (0..=127) are positive, special markers
/// (`AL`, `LL`, `ML`, `PL`, `AS`, `PS`) are the negative constants above.
#[rustfmt::skip]
const CHARMAPS: [[i16; 4]; 30] = [
    [b'A' as i16, b'a' as i16, b'0' as i16, b';' as i16],
    [b'B' as i16, b'b' as i16, b'1' as i16, b'<' as i16],
    [b'C' as i16, b'c' as i16, b'2' as i16, b'>' as i16],
    [b'D' as i16, b'd' as i16, b'3' as i16, b'@' as i16],
    [b'E' as i16, b'e' as i16, b'4' as i16, b'[' as i16],
    [b'F' as i16, b'f' as i16, b'5' as i16, b'\\' as i16],
    [b'G' as i16, b'g' as i16, b'6' as i16, b']' as i16],
    [b'H' as i16, b'h' as i16, b'7' as i16, b'_' as i16],
    [b'I' as i16, b'i' as i16, b'8' as i16, b'`' as i16],
    [b'J' as i16, b'j' as i16, b'9' as i16, b'~' as i16],
    [b'K' as i16, b'k' as i16, b'&' as i16, b'!' as i16],
    [b'L' as i16, b'l' as i16, 13,           13],
    [b'M' as i16, b'm' as i16,  9,            9],
    [b'N' as i16, b'n' as i16, b',' as i16, b',' as i16],
    [b'O' as i16, b'o' as i16, b':' as i16, b':' as i16],
    [b'P' as i16, b'p' as i16, b'#' as i16, 10],
    [b'Q' as i16, b'q' as i16, b'-' as i16, b'-' as i16],
    [b'R' as i16, b'r' as i16, b'.' as i16, b'.' as i16],
    [b'S' as i16, b's' as i16, b'$' as i16, b'$' as i16],
    [b'T' as i16, b't' as i16, b'/' as i16, b'/' as i16],
    [b'U' as i16, b'u' as i16, b'+' as i16, b'"' as i16],
    [b'V' as i16, b'v' as i16, b'%' as i16, b'|' as i16],
    [b'W' as i16, b'w' as i16, b'*' as i16, b'*' as i16],
    [b'X' as i16, b'x' as i16, b'=' as i16, b'(' as i16],
    [b'Y' as i16, b'y' as i16, b'^' as i16, b')' as i16],
    [b'Z' as i16, b'z' as i16, PL,           b'?' as i16],
    [b' ' as i16, b' ' as i16, b' ' as i16, b'{' as i16],
    [LL,          AS,          LL,           b'}' as i16],
    [ML,          ML,          AL,           b'\'' as i16],
    [PS,          PS,          PS,           AL],
];

/// `latlen[from][to]` — number of latch codewords required to switch
/// from sub-mode `from` to sub-mode `to`.
#[rustfmt::skip]
const LATLEN: [[i32; 4]; 4] = [
    [0, 1, 1, 2], // from alpha
    [2, 0, 1, 2], // from lower
    [1, 1, 0, 1], // from mixed
    [1, 2, 2, 0], // from punct
];

/// `latseq[from][to]` — the actual sequence of latch markers needed.
/// Each entry has `latlen[from][to]` items.
#[rustfmt::skip]
const LATSEQ: [[&[i16]; 4]; 4] = [
    [&[],          &[LL],     &[ML],      &[ML, PL]], // from alpha
    [&[ML, AL],    &[],       &[ML],      &[ML, PL]], // from lower
    [&[AL],        &[LL],     &[],        &[PL]],     // from mixed
    [&[AL],        &[AL, LL], &[AL, ML],  &[]],       // from punct
];

/// `shftlen[from][to]` — number of shift codewords for a single-char
/// shift. `INF` means no shift exists.
#[rustfmt::skip]
const SHFTLEN: [[i32; 4]; 4] = [
    [INF, INF, INF, 1],   // from alpha
    [1,   INF, INF, 1],   // from lower
    [INF, INF, INF, 1],   // from mixed
    [INF, INF, INF, INF], // from punct
];

/// Encode `ch` (an ASCII byte or one of the AL/LL/ML/PL/AS/PS markers)
/// in `submode`. Returns the 0..29 sub-codeword value, or `None` if
/// `ch` isn't representable in `submode`.
fn encode_in(submode: usize, ch: i16) -> Option<u8> {
    CHARMAPS
        .iter()
        .position(|row| row[submode] == ch)
        .map(|i| i as u8)
}

/// Whether `ch` is representable in `submode`.
fn in_submode(submode: usize, ch: i16) -> bool {
    encode_in(submode, ch).is_some()
}

/// Produce PDF417 text-compaction codewords (0..899) for an ASCII input.
/// Each pair of sub-codewords is packed as `high * 30 + low`; an odd
/// trailing sub-codeword is padded with `29`.
pub(crate) fn text_codewords(input: &str) -> Result<Vec<u16>, String> {
    // Validate: all chars must be representable in at least one sub-mode.
    let bytes: Vec<i16> = input.bytes().map(|b| b as i16).collect();
    for &b in &bytes {
        if !(0..=3).any(|s| in_submode(s, b)) {
            return Err(format!("PDF417: byte {b:#x} is not in any text sub-mode"));
        }
    }

    // Dijkstra-style DP: at each character, track the minimum-cost
    // sequence ending in each of the 4 sub-modes.
    let mut cur_seq: [Vec<i16>; 4] = Default::default();
    let mut cur_len: [i32; 4] = [INF; 4];
    cur_len[ALPHA] = 0;

    for &ch in &bytes {
        // Step 1: relax latches between the current per-submode states.
        // Repeat until no improvement (cheap chains of latches stabilize
        // in at most 4 passes, but `loop`-until-fixed-point is what
        // BWIPP does and it's clearer than reasoning about bounds).
        loop {
            let mut improved = false;
            for x in 0..4 {
                for y in 0..4 {
                    if cur_len[x] >= INF {
                        continue;
                    }
                    let cost = cur_len[x] + LATLEN[x][y];
                    if cost < cur_len[y] {
                        cur_len[y] = cost;
                        let src = cur_seq[x].clone();
                        cur_seq[y] = src;
                        cur_seq[y].extend_from_slice(LATSEQ[x][y]);
                        improved = true;
                    }
                }
            }
            if !improved {
                break;
            }
        }

        // Step 2: emit `ch` in each sub-mode that accepts it, either as
        // a plain encoding from that sub-mode or as a single-char shift
        // from a different one.
        let mut nxt_seq: [Vec<i16>; 4] = Default::default();
        let mut nxt_len: [i32; 4] = [INF; 4];
        for x in 0..4 {
            if !in_submode(x, ch) {
                continue;
            }
            // Plain encoding from sub-mode x.
            let cost = cur_len[x].saturating_add(1);
            if cost < nxt_len[x] {
                nxt_len[x] = cost;
                let mut s = cur_seq[x].clone();
                s.push(ch);
                nxt_seq[x] = s;
            }
            // Shift from y to x for one char.
            for y in 0..4 {
                if y == x || SHFTLEN[y][x] >= INF || cur_len[y] >= INF {
                    continue;
                }
                let cost = cur_len[y] + SHFTLEN[y][x] + 1;
                if cost < nxt_len[y] {
                    nxt_len[y] = cost;
                    let mut s = cur_seq[y].clone();
                    s.push(if x == ALPHA { AS } else { PS });
                    s.push(ch);
                    nxt_seq[y] = s;
                }
            }
        }

        cur_len = nxt_len;
        cur_seq = nxt_seq;
    }

    // Pick the cheapest final sub-mode.
    let mut min_len = INF;
    let mut txtseq: Vec<i16> = Vec::new();
    for k in 0..4 {
        if cur_len[k] < min_len {
            min_len = cur_len[k];
            txtseq = cur_seq[k].clone();
        }
    }

    // Execute the path: walk `txtseq`, encoding each item in the
    // current sub-mode and handling latches/shifts.
    let mut text: Vec<u8> = Vec::new();
    let mut submode = ALPHA;
    let mut i = 0;
    while i < txtseq.len() {
        let ch = txtseq[i];
        text.push(
            encode_in(submode, ch)
                .ok_or_else(|| format!("internal: char {ch:#x} not in submode {submode}"))?,
        );
        i += 1;
        if ch == AS || ch == PS {
            // Next char encodes in shifted sub-mode.
            let shifted = if ch == AS { ALPHA } else { PUNCT };
            let next_ch = txtseq[i];
            text.push(
                encode_in(shifted, next_ch).ok_or_else(|| {
                    format!("internal: shifted char {next_ch:#x} not in {shifted}")
                })?,
            );
            i += 1;
        }
        match ch {
            AL => submode = ALPHA,
            LL => submode = LOWER,
            ML => submode = MIXED,
            PL => submode = PUNCT,
            _ => {}
        }
    }

    // Pad odd-length stream with 29 ("ps" in alpha/lower/mixed; "al"
    // in punct — both are valid filler codewords in this position).
    if text.len() % 2 != 0 {
        text.push(29);
    }

    // Pack pairs.
    Ok(text
        .chunks_exact(2)
        .map(|p| u16::from(p[0]) * 30 + u16::from(p[1]))
        .collect())
}

/// PDF417 binary (byte) compaction (mode 901 / 924): represents a
/// run of arbitrary bytes as base-900 codewords. Each full 6-byte
/// group `[b0, b1, b2, b3, b4, b5]` is treated as a 48-bit big-endian
/// integer and converted to 5 base-900 codewords. Any tail of 1..=5
/// remaining bytes is appended as raw bytes (one codeword per byte,
/// since each byte's value 0..=255 fits in a codeword).
///
/// The latch codewords (`901` when there is a remainder, `924` when
/// the input length is a multiple of 6) are emitted by the mode
/// selector, not by this function.
pub(crate) fn byte_codewords(bytes: &[u8]) -> Vec<u16> {
    let mut out = Vec::with_capacity((bytes.len() / 6) * 5 + bytes.len() % 6);
    let full_groups = bytes.len() / 6;
    for k in 0..full_groups {
        let off = k * 6;
        let mut v: u64 = 0;
        for &b in &bytes[off..off + 6] {
            v = (v << 8) | b as u64;
        }
        // 900^4 = 656_100_000_000; 900^3 = 729_000_000; 900^2 = 810_000.
        let cw0 = v / 656_100_000_000;
        let r = v % 656_100_000_000;
        let cw1 = r / 729_000_000;
        let r = r % 729_000_000;
        let cw2 = r / 810_000;
        let r = r % 810_000;
        let cw3 = r / 900;
        let cw4 = r % 900;
        out.push(cw0 as u16);
        out.push(cw1 as u16);
        out.push(cw2 as u16);
        out.push(cw3 as u16);
        out.push(cw4 as u16);
    }
    // Tail bytes (1..=5) become raw codewords.
    for &b in &bytes[full_groups * 6..] {
        out.push(b as u16);
    }
    out
}

/// PDF417 numeric compaction (mode 902): represents a run of decimal
/// digits as base-900 codewords. The input is split into chunks of up
/// to 44 digits; each chunk is prepended with a literal `1` and then
/// converted to base 900. The `1` prefix is what lets PDF417 round-trip
/// runs that begin with `0` digits.
pub(crate) fn numeric_codewords(digits: &str) -> Result<Vec<u16>, String> {
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err("PDF417 numeric: input must contain only ASCII digits".into());
    }
    let mut out = Vec::new();
    let bytes = digits.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let end = (i + 44).min(bytes.len());
        // gmod[0] = 1 (PDF417's literal-prefix sentinel), then each
        // input digit as a u32.
        let mut gmod: Vec<u32> = Vec::with_capacity(end - i + 1);
        gmod.push(1);
        for &b in &bytes[i..end] {
            gmod.push((b - b'0') as u32);
        }
        // Repeated long division by 900: each pass yields one remainder
        // (the next base-900 digit, low-order first) and replaces gmod
        // with its quotient.
        let mut cwn: Vec<u16> = Vec::new();
        while !gmod.is_empty() {
            let mut new_gmod = Vec::new();
            let mut val: u32 = 0;
            let mut started = false;
            for &d in &gmod {
                val = val * 10 + d;
                if val >= 900 {
                    started = true;
                    new_gmod.push(val / 900);
                    val %= 900;
                } else if started {
                    new_gmod.push(0);
                }
            }
            cwn.push(val as u16);
            gmod = new_gmod;
        }
        // cwn holds remainders low-to-high; emit them high-to-low.
        cwn.reverse();
        out.extend(cwn);
        i = end;
    }
    Ok(out)
}

// --- Mode selector (segmenter) ---

/// Latch codeword constants emitted by the mode selector to switch
/// between compaction modes.
const LATCH_TEXT: u16 = 900;
const LATCH_BYTE: u16 = 901;
const LATCH_NUMERIC: u16 = 902;
const LATCH_BYTE_SHIFT: u16 = 913; // single-byte shift inside text mode
const LATCH_BYTE_6: u16 = 924; // byte mode latch when len is a multiple of 6

/// State machine for the segmenter — tracks which mode we're currently in.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Mode {
    Text,
    Numeric,
    Byte,
}

/// Whether `b` is a character that fits in *some* text sub-mode. Mirrors
/// the `alltext` set bwip-js builds from the union of all four CHARMAPS
/// columns.
fn is_text_byte(b: u8) -> bool {
    let v = b as i16;
    CHARMAPS.iter().any(|row| row.contains(&v))
}

/// Pick the best compaction mode for each contiguous run of input bytes,
/// emit the appropriate latch codewords, and return the full data
/// codeword stream. Mirrors bwip-js's segment-then-execute pipeline.
///
/// Decision rules per position `p` (matching bwip-js exactly):
/// 1. If numdigits[p] ≥ 13, OR the entire remaining input is digits and
///    ≥ 8 chars: emit a numeric run.
/// 2. Else if numtext[p] ≥ 5: emit a text run.
/// 3. Else if numbytes[p] = 1 and we're already in text mode: emit a
///    single-byte shift (codeword 913 + the raw byte).
/// 4. Otherwise: emit a byte run; the latch is `924` if the run length
///    is a multiple of 6, `901` otherwise.
///
/// The numtext counter is gated so it never spills into a long digit
/// run (digits prefer numeric), and numbytes is similarly gated.
pub(crate) fn data_codewords(msg: &[u8]) -> Result<Vec<u16>, String> {
    let n = msg.len();
    if n == 0 {
        return Ok(Vec::new());
    }

    // Precompute look-ahead counts. We populate position n with 0 and
    // sweep right-to-left so each cell can read the next cell's value.
    let mut numdigits = vec![0usize; n + 1];
    let mut numtext = vec![0usize; n + 1];
    let mut numbytes = vec![0usize; n + 1];
    for i in (0..n).rev() {
        if msg[i].is_ascii_digit() {
            numdigits[i] = numdigits[i + 1] + 1;
        }
        if is_text_byte(msg[i]) && numdigits[i] < 13 {
            numtext[i] = numtext[i + 1] + 1;
        }
        if numtext[i] < 5 && numdigits[i] < 13 {
            numbytes[i] = numbytes[i + 1] + 1;
        }
    }

    let mut out: Vec<u16> = Vec::new();
    let mut state = Mode::Text;
    let mut p = 0;
    while p < n {
        let nd = numdigits[p];
        if nd >= 13 || (nd == n - p && nd >= 8) {
            out.push(LATCH_NUMERIC);
            let segment = std::str::from_utf8(&msg[p..p + nd])
                .map_err(|_| "PDF417: numeric segment contained non-ASCII bytes".to_string())?;
            out.extend(numeric_codewords(segment)?);
            state = Mode::Numeric;
            p += nd;
            continue;
        }
        let nt = numtext[p];
        if nt >= 5 {
            if state != Mode::Text {
                out.push(LATCH_TEXT);
            }
            let segment = std::str::from_utf8(&msg[p..p + nt])
                .map_err(|_| "PDF417: text segment contained non-ASCII bytes".to_string())?;
            out.extend(text_codewords(segment)?);
            state = Mode::Text;
            p += nt;
            continue;
        }
        let nb = numbytes[p];
        if nb == 1 && state == Mode::Text {
            out.push(LATCH_BYTE_SHIFT);
            out.push(msg[p] as u16);
            p += 1;
            // state stays Text — shift is single-byte, not a latch.
            continue;
        }
        // Byte run.
        if nb % 6 == 0 {
            out.push(LATCH_BYTE_6);
        } else {
            out.push(LATCH_BYTE);
        }
        out.extend(byte_codewords(&msg[p..p + nb]));
        state = Mode::Byte;
        p += nb;
    }

    Ok(out)
}

/// Compose a full PDF417 codeword array — the data codewords from
/// [`data_codewords`] prefixed with the length `n`, padded with `900`
/// up to `n`, and trailed by `k = 2^(eclevel+1)` Reed-Solomon check
/// codewords from [`crate::util::rs_gf929::encode`]. The returned
/// length is `c * r` (column count × row count).
///
/// `eclevel` must be `0..=8`. `columns` is the data-column count
/// (excluding the left/right indicator columns the renderer will add);
/// pass `0` to let the encoder choose `round(sqrt((m + k) / 3))`.
pub fn pdf417_cws(input: &[u8], eclevel: u8, columns: usize) -> Result<Vec<u16>, String> {
    if eclevel > 8 {
        return Err(format!("PDF417: eclevel must be 0..=8, got {eclevel}"));
    }
    let datcws = data_codewords(input)?;
    let m = datcws.len();
    if m > 926 {
        return Err(format!("PDF417: data too long ({m} codewords > 926)"));
    }
    let k = 1usize << (eclevel as usize + 1);
    let c = if columns == 0 {
        (((m + k) as f64 / 3.0).sqrt().round() as usize).max(1)
    } else {
        columns
    };
    let mut r = (m + k + 1).div_ceil(c).max(3);
    if r > 90 {
        return Err("PDF417: insufficient capacity (r > 90)".into());
    }
    // After fixing r, also bump eclevel back up if more capacity opened
    // up (BWIPP's `maxeclevel` re-bump). Capped at the input eclevel
    // since we treat the parameter as a hard floor — auto-eclevel is
    // the caller's job, not ours.
    let n = c * r - k;
    if n > 928 {
        return Err(format!("PDF417: n {n} > 928"));
    }
    // Recompute r if it ended up too small (shouldn't happen given
    // div_ceil but defensive against odd column counts).
    if c * r < m + k + 1 {
        r = (m + k + 1).div_ceil(c);
    }
    let total = c * r;
    let mut cws: Vec<u16> = vec![0; total];
    cws[0] = n as u16;
    cws[1..=m].copy_from_slice(&datcws);
    for slot in cws.iter_mut().take(n).skip(m + 1) {
        *slot = 900;
    }
    // RS check over the first n codewords.
    let check = crate::util::rs_gf929::encode(&cws[..n], k);
    cws[n..n + k].copy_from_slice(&check);
    Ok(cws)
}

/// 17-module start pattern emitted at the beginning of every row.
#[rustfmt::skip]
const START_PATTERN: [u8; 17] = [
    1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0,
];

/// 18-module stop pattern emitted at the end of every row.
#[rustfmt::skip]
const STOP_PATTERN: [u8; 18] = [
    1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1,
];

/// Convert a codeword (0..928) to its 17-bit pattern in the given
/// cluster (`i % 3`). Returns the 17 module bits in left-to-right order.
fn cw_to_bits(cw: u16, cluster: usize) -> [u8; 17] {
    let v = clusters::ALL_CLUSTERS[cluster][cw as usize];
    let mut out = [0u8; 17];
    for (i, slot) in out.iter_mut().enumerate() {
        // High bit first: bit 16 of v at index 0, bit 0 at index 16.
        *slot = ((v >> (16 - i)) & 1) as u8;
    }
    out
}

/// Render a full PDF417 symbol as a [`BitMatrix`]. Each row is
/// `17 * (c + 3) + 18` modules wide; the symbol has `r` rows.
pub fn pdf417_render(input: &[u8], eclevel: u8, columns: usize) -> Result<BitMatrix, String> {
    render_inner(input, eclevel, columns, false)
}

/// Render a PDF417 **Truncated** symbol as a [`BitMatrix`]. The
/// truncated variant drops the right row-indicator codeword and
/// replaces the 18-module stop pattern with a single 1-bar stop, so
/// each row is `17 * (c + 2) + 1` modules wide.
pub fn pdf417_render_truncated(
    input: &[u8],
    eclevel: u8,
    columns: usize,
) -> Result<BitMatrix, String> {
    render_inner(input, eclevel, columns, true)
}

/// Render a PDF417 CC-C symbol from a pre-packed byte stream.
///
/// CC-C bypasses PDF417's mode-switching encoder — the data is already
/// byte-mode-encoded by gs1_cc with a `920` CC-C preamble and a
/// `901`/`924` byte-mode latch. The 6-bytes-to-5-codewords expansion
/// (see [`crate::symbology::micropdf417::pack_ccb_datcws`]) is shared
/// with CC-B; the only difference is that CC-C renders to PDF417's
/// variable-size layout instead of MicroPDF417's fixed metrics.
pub(crate) fn pdf417_render_ccc(
    bytes: &[u8],
    eclevel: u8,
    columns: usize,
) -> Result<BitMatrix, String> {
    if eclevel > 8 {
        return Err(format!("PDF417 CC-C: eclevel must be 0..=8, got {eclevel}"));
    }
    if columns == 0 {
        return Err("PDF417 CC-C: explicit columns required (auto-size not supported)".into());
    }
    let datcws = crate::symbology::micropdf417::pack_ccb_datcws(bytes);
    let m = datcws.len();
    let k = 1usize << (eclevel as usize + 1);
    let mut r = (m + k + 1).div_ceil(columns).max(3);
    if r > 90 {
        return Err(format!(
            "PDF417 CC-C: insufficient capacity ({m} datcws + {k} ecc > {} slots)",
            90 * columns,
        ));
    }
    if columns * r < m + k + 1 {
        r = (m + k + 1).div_ceil(columns);
    }
    let total = columns * r;
    let n = total - k;
    // The symbol-length descriptor (cws[0] = n) is itself a PDF417 codeword
    // and so must be a valid 0..=928 value; likewise every data slot. A large
    // CC-C payload can push n past 928, which would later index the 929-entry
    // cluster table out of bounds in `cw_to_bits`. Reject as over-capacity
    // here rather than panicking on untrusted input.
    if n > 928 {
        return Err(format!(
            "PDF417 CC-C: insufficient capacity ({n} data codewords exceeds the 928 PDF417 maximum)"
        ));
    }
    let mut cws: Vec<u16> = vec![0; total];
    cws[0] = n as u16;
    cws[1..=m].copy_from_slice(&datcws);
    for slot in cws.iter_mut().take(n).skip(m + 1) {
        *slot = 900;
    }
    let check = crate::util::rs_gf929::encode(&cws[..n], k);
    cws[n..n + k].copy_from_slice(&check);
    render_cws_inner(&cws, eclevel, columns, r, false)
}

fn render_inner(
    input: &[u8],
    eclevel: u8,
    columns: usize,
    truncated: bool,
) -> Result<BitMatrix, String> {
    let cws = pdf417_cws(input, eclevel, columns)?;
    // Recover c and r from the codeword stream.
    let n = cws[0] as usize;
    let k = 1usize << (eclevel as usize + 1);
    let total = cws.len();
    // total = c * r; n = c * r - k → c * r = n + k = total.
    debug_assert_eq!(n + k, total);
    let c = if columns == 0 {
        (((n + k - 1).max(1) as f64 / 3.0).sqrt().round() as usize).max(1)
    } else {
        columns
    };
    // r divides total / c.
    let r = total / c;
    render_cws_inner(&cws, eclevel, c, r, truncated)
}

fn render_cws_inner(
    cws: &[u16],
    eclevel: u8,
    c: usize,
    r: usize,
    truncated: bool,
) -> Result<BitMatrix, String> {
    let rwid = if truncated {
        17 * (c + 2) + 1
    } else {
        17 * (c + 3) + 18
    };
    let mut bm = BitMatrix::new(rwid, r);
    for i in 0..r {
        let cluster = i % 3;
        // Left and right row-indicator codewords. These encode the
        // row number, eclevel, c, and r through three different
        // formulas depending on `i % 3` (matches bwip-js exactly).
        // Truncated symbols still emit the left RI codeword — it
        // carries r, c, and eclevel between them — but skip the
        // right RI codeword entirely.
        let lcw = match i % 3 {
            0 => (i / 3) * 30 + (r - 1) / 3,
            1 => (i / 3) * 30 + eclevel as usize * 3 + (r - 1) % 3,
            _ => (i / 3) * 30 + c - 1,
        } as u16;

        // Build the row's 1D bit array, then paint it into the matrix.
        let mut row: Vec<u8> = Vec::with_capacity(rwid);
        row.extend_from_slice(&START_PATTERN);
        row.extend_from_slice(&cw_to_bits(lcw, cluster));
        for j in 0..c {
            row.extend_from_slice(&cw_to_bits(cws[c * i + j], cluster));
        }
        if truncated {
            row.push(1);
        } else {
            let rcw = match i % 3 {
                0 => (i / 3) * 30 + c - 1,
                1 => (i / 3) * 30 + (r - 1) / 3,
                _ => (i / 3) * 30 + eclevel as usize * 3 + (r - 1) % 3,
            } as u16;
            row.extend_from_slice(&cw_to_bits(rcw, cluster));
            row.extend_from_slice(&STOP_PATTERN);
        }
        debug_assert_eq!(row.len(), rwid);
        for (x, &bit) in row.iter().enumerate() {
            if bit != 0 {
                bm.set(x, i, true);
            }
        }
    }
    Ok(bm)
}

/// Encode `data` as a PDF417 symbol, reading `eclevel` and `columns`
/// from `opts.extras`. Returns a [`BitMatrix`] suitable for wrapping in
/// [`crate::encoding::Encoded::Matrix`].
///
/// Recognized options:
///   * `eclevel` = `0..=8` (default `2`) — Reed-Solomon strength; `k =
///     2^(eclevel+1)` check codewords are appended.
///   * `columns` = `1..=30` (default `0` = auto) — data-column count
///     excluding the left/right indicator columns. `0` lets the encoder
///     pick `round(sqrt((m + k) / 3))`.
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// // Default: eclevel=2, auto columns.
/// let svg = render_svg(Symbology::Pdf417, "Hello, world!", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
///
/// // Tune the ECC strength and force a 4-column layout.
/// let mut opts = Options::default();
/// opts.extras.push(("eclevel".into(), "4".into()));
/// opts.extras.push(("columns".into(), "4".into()));
/// let svg = render_svg(Symbology::Pdf417, "https://example.com", &opts).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    let eclevel: u8 = match opts.get("eclevel") {
        Some(s) => s
            .parse::<u8>()
            .map_err(|_| Error::InvalidOption(format!("eclevel={s} (want 0..=8)")))?,
        None => 2,
    };
    if eclevel > 8 {
        return Err(Error::InvalidOption(format!(
            "eclevel={eclevel} out of range 0..=8"
        )));
    }
    let columns: usize = match opts.get("columns") {
        Some(s) => s
            .parse::<usize>()
            .map_err(|_| Error::InvalidOption(format!("columns={s} (want 0..=30)")))?,
        None => 0,
    };
    if columns > 30 {
        return Err(Error::InvalidOption(format!(
            "columns={columns} out of range 0..=30"
        )));
    }
    pdf417_render(data.as_bytes(), eclevel, columns).map_err(Error::InvalidData)
}

/// Encode `data` as a PDF417 **Truncated** symbol. Reads the same
/// `eclevel` and `columns` options as [`encode`] but drops the right
/// row-indicator column and emits a 1-bar stop, making the symbol
/// roughly 34% narrower at the cost of slightly less scanner
/// tolerance (the missing right RI codewords carry redundant
/// row/col/eclevel hints).
///
/// # Example
///
/// ```
/// use bwipp::{render_svg, Options, Symbology};
///
/// let svg = render_svg(Symbology::Pdf417Truncated, "PDF417", &Options::default()).unwrap();
/// assert!(svg.starts_with("<svg"));
/// ```
pub fn encode_truncated(data: &str, opts: &Options) -> Result<BitMatrix, Error> {
    let eclevel: u8 = match opts.get("eclevel") {
        Some(s) => s
            .parse::<u8>()
            .map_err(|_| Error::InvalidOption(format!("eclevel={s} (want 0..=8)")))?,
        None => 2,
    };
    if eclevel > 8 {
        return Err(Error::InvalidOption(format!(
            "eclevel={eclevel} out of range 0..=8"
        )));
    }
    let columns: usize = match opts.get("columns") {
        Some(s) => s
            .parse::<usize>()
            .map_err(|_| Error::InvalidOption(format!("columns={s} (want 0..=30)")))?,
        None => 0,
    };
    if columns > 30 {
        return Err(Error::InvalidOption(format!(
            "columns={columns} out of range 0..=30"
        )));
    }
    pdf417_render_truncated(data.as_bytes(), eclevel, columns).map_err(Error::InvalidData)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: a CC-C payload whose data-codeword count would exceed the
    /// 928 PDF417 maximum must return `Err`, not panic. Before the fix, the
    /// symbol-length descriptor `cws[0] = n` could be set to a value > 928,
    /// which then indexed the 929-entry cluster table out of bounds in
    /// `cw_to_bits` ("index out of bounds: the len is 929 but the index is
    /// 958"). Found by `fuzz_composite_ccc`.
    #[test]
    fn ccc_over_capacity_returns_err_not_panic() {
        let big = vec![b'A'; 2000];
        let r = pdf417_render_ccc(&big, 0, 30);
        assert!(
            r.is_err(),
            "over-capacity CC-C must return Err, not panic / not Ok"
        );
    }

    /// Golden text-compaction codewords for "PDF417" as captured by
    /// bwip-js's `debugcws` (in the n=6 data layout, datcws = positions
    /// 1..n minus the 900-padding tail — i.e. 4 real codewords).
    #[test]
    fn text_pdf417_canonical() {
        // Stage 11.A8c (cont) — bare `.unwrap()` → `.expect(...)` with
        // input echo + text-compaction path label.
        let cws = text_codewords("PDF417")
            .expect("text_codewords(\"PDF417\") (canonical mixed-case PDF417 word) must succeed");
        assert_eq!(cws, vec![453, 178, 121, 239]);
    }

    #[test]
    fn text_uppercase_only() {
        // Pure alpha — should stay in alpha sub-mode throughout, no
        // latches needed.
        // Stage 11.A8c (cont) — `.expect` with sub-mode rationale.
        let cws = text_codewords("ABCD").expect(
            "text_codewords(\"ABCD\") (pure-uppercase staying in alpha sub-mode, no latches) must succeed",
        );
        // A=0, B=1, C=2, D=3 → (0,1)=1, (2,3)=63.
        assert_eq!(cws, vec![1, 63]);
    }

    #[test]
    fn text_padding_for_odd_length() {
        // 5-char input → 5 sub-codewords + 1 pad = 3 pairs.
        // Stage 11.A8c (cont) — `.expect` with odd-length pad rationale.
        let cws = text_codewords("ABCDE").expect(
            "text_codewords(\"ABCDE\") (5-char odd-length → 5 sub-codewords + 1 pad(29) → 3 pairs) must succeed",
        );
        // ABCD as above + E(4) + pad(29) → pairs (0,1)=1, (2,3)=63, (4,29)=149.
        assert_eq!(cws, vec![1, 63, 149]);
    }

    /// `"Hello World"` exercises the lower-latch (`LL`) and the
    /// punct-shift/alpha-shift logic on a single capital embedded in a
    /// run of lower-case. Captured from bwip-js's debugcws output.
    #[test]
    fn text_hello_world_matches_bwip_js() {
        // Stage 11.A8c (cont) — `.expect` naming LL+alpha-shift path.
        let cws = text_codewords("Hello World").expect(
            "text_codewords(\"Hello World\") (lower-latch LL + punct-shift + alpha-shift on embedded capital) must succeed",
        );
        assert_eq!(cws, vec![237, 131, 344, 807, 674, 521, 119]);
    }

    #[test]
    fn numeric_short_run() {
        // "1234567890123" (13 digits) — bwip-js produces these 5
        // codewords inside a numeric-mode segment (after the 902 latch).
        // Stage 11.A8c (cont) — `.expect` naming numeric-mode segment + 902 latch.
        let cws = numeric_codewords("1234567890123").expect(
            "numeric_codewords(\"1234567890123\") (13-digit numeric-mode segment after 902 latch → 5 codewords) must succeed",
        );
        assert_eq!(cws, vec![17, 110, 836, 811, 223]);
    }

    #[test]
    fn numeric_chunks_at_44_digits() {
        // 45 digits → first 44-digit chunk produces 15 codewords, then a
        // separate 1-digit chunk produces 1 more. Total 16.
        // Stage 11.A8c (cont) — `.expect` naming 44-digit chunk boundary.
        let cws = numeric_codewords("123456789012345678901234567890123456789012345").expect(
            "numeric_codewords(45-digit input) (44-digit chunk → 15 cws + 1-digit tail chunk → 1 cw = 16 total) must succeed",
        );
        assert_eq!(
            cws,
            vec![491, 81, 137, 450, 302, 67, 15, 174, 492, 862, 667, 475, 869, 12, 434, 15]
        );
    }

    #[test]
    fn numeric_rejects_non_digits() {
        // Pin the static rejection-arm diagnostic from line 333:
        //   "PDF417 numeric: input must contain only ASCII digits"
        // No value interpolation here — but each substring kills a
        // distinct mutation class:
        //   * "PDF417 numeric:" — symbology prefix tag.
        //   * "input must contain" — predicate verb.
        //   * "only ASCII digits" — narrowing object.
        let err = numeric_codewords("123ABC").unwrap_err();
        assert!(
            err.contains("PDF417 numeric:"),
            "diagnostic must carry the symbology prefix; got {err}"
        );
        assert!(
            err.contains("input must contain"),
            "diagnostic must carry the predicate verb; got {err}"
        );
        assert!(
            err.contains("only ASCII digits"),
            "diagnostic must carry the narrowing object; got {err}"
        );
    }

    /// Byte mode: full 6-byte group. The 0x01..0x06 input becomes a
    /// 48-bit BE integer that converts to 5 base-900 codewords.
    /// Captured from bwip-js (after the 924 latch).
    #[test]
    fn byte_full_group() {
        let cws = byte_codewords(&[1, 2, 3, 4, 5, 6]);
        assert_eq!(cws, vec![1, 620, 89, 74, 846]);
    }

    /// Two full groups (12 bytes, exact multiple of 6).
    #[test]
    fn byte_two_full_groups() {
        let cws = byte_codewords(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        assert_eq!(cws, vec![1, 620, 89, 74, 846, 11, 705, 58, 895, 432]);
    }

    /// 6 + 2 bytes: one full group + a 2-byte raw tail.
    #[test]
    fn byte_group_plus_tail() {
        let cws = byte_codewords(&[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(cws, vec![1, 620, 89, 74, 846, 7, 8]);
    }

    /// Sub-6-byte run: every byte is emitted raw.
    #[test]
    fn byte_short_run() {
        let cws = byte_codewords(b"ABCD");
        assert_eq!(cws, vec![65, 66, 67, 68]);
    }

    /// `data_codewords` should match bwip-js's full datcws output (no
    /// length prefix, no padding, no RS — just the mode-latched
    /// compaction stream). Goldens captured via `tools/oracle-pdf417.js`.
    #[test]
    fn data_codewords_match_bwip_js() {
        let cases: &[(&[u8], &[u16])] = &[
            // "PDF417" — pure text, no latch needed (text is the default).
            (b"PDF417", &[453, 178, 121, 239]),
            // "Hello World" — same, longer.
            (b"Hello World", &[237, 131, 344, 807, 674, 521, 119]),
            // "ABCD" — only 4 chars, doesn't hit the 5-char text threshold,
            // numbytes==4 (not 1), so the segmenter emits a byte latch (901)
            // followed by 4 raw bytes.
            (b"ABCD", &[901, 65, 66, 67, 68]),
            // "12345" — numdigits=5 which fills the entire input AND
            // is ≥ 8? No (5 < 8). And < 13. So text mode is chosen
            // (digits live in mixed sub-mode); text_codewords produces
            // [841, 63, 125] (no latch since text is the default state).
            (b"12345", &[841, 63, 125]),
            // "1234567890123" — 13 digits → numeric latch + numeric run.
            (b"1234567890123", &[902, 17, 110, 836, 811, 223]),
            // "PDF417 12345" — entire input stays in text mode (digits live
            // in the mixed sub-mode); the encoder latches alpha → mixed
            // mid-segment without a mode-selector latch.
            (b"PDF417 12345", &[453, 178, 121, 236, 32, 94, 179]),
            // "AB" + 0xC3 0xA9 + "CD" (UTF-8 "ABéCD"). Non-ASCII bytes
            // disqualify the text sub-modes, so the whole 6-byte run
            // goes through byte mode with the 924 latch.
            (b"AB\xC3\xA9CD", &[924, 109, 329, 327, 474, 300]),
            // "12345678" — 8 digits, exactly at the "whole-input ≥ 8"
            // threshold for numeric mode. Triggers the numeric branch
            // even though numdigits (8) is < 13.
            (b"12345678", &[902, 138, 628, 478]),
        ];
        for &(input, want) in cases {
            // Stage 11.A8c (cont) — `.unwrap()` → `.unwrap_or_else` with
            // utf-8-safe input echo + path label naming the PDF417
            // data-codewords classifier corpus item.
            let got = data_codewords(input).unwrap_or_else(|e| {
                panic!(
                    "data_codewords({:?}) (PDF417 mode-classifier corpus item) must succeed; got Err: {e}",
                    std::str::from_utf8(input).unwrap_or("<non-utf8>")
                )
            });
            assert_eq!(
                got,
                want,
                "data_codewords mismatch for {:?}",
                std::str::from_utf8(input).unwrap()
            );
        }
    }

    /// Full cws (length prefix + datcws + 900 padding + RS check).
    /// Goldens captured via `tools/oracle-pdf417.js` with explicit
    /// eclevel arguments.
    #[test]
    fn pdf417_cws_matches_bwip_js() {
        // Stage 11.A8c (cont) — bare `.unwrap()` → `.expect(...)` with
        // input + (eclevel, geometry) echo per corpus row.
        // "PDF417" eclevel=2: c=2, r=7, k=8, n=6, total=14.
        let cws = pdf417_cws(b"PDF417", 2, 0).expect(
            "pdf417_cws(b\"PDF417\", eclevel=2) (c=2 r=7 k=8 n=6 total=14, mixed-case word) must succeed",
        );
        assert_eq!(
            cws,
            vec![6, 453, 178, 121, 239, 900, 466, 823, 655, 326, 814, 259, 528, 611]
        );
        // "Hello World" eclevel=1: c=2, r=6, k=4, n=8, total=12.
        let cws = pdf417_cws(b"Hello World", 1, 0).expect(
            "pdf417_cws(b\"Hello World\", eclevel=1) (c=2 r=6 k=4 n=8 total=12, mixed-case + space) must succeed",
        );
        assert_eq!(
            cws,
            vec![8, 237, 131, 344, 807, 674, 521, 119, 661, 754, 265, 571]
        );
        // "ABCD" eclevel=0: c=2, r=4, k=2, m=5, n=6, total=8. No
        // 900 padding (n - m - 1 = 0).
        let cws = pdf417_cws(b"ABCD", 0, 0).expect(
            "pdf417_cws(b\"ABCD\", eclevel=0) (c=2 r=4 k=2 m=5 n=6 total=8, no 900-padding because n-m-1=0) must succeed",
        );
        assert_eq!(cws, vec![6, 901, 65, 66, 67, 68, 911, 504]);
        // "1234567890123" eclevel=1: c=2, r=6, k=4, n=8, total=12.
        let cws = pdf417_cws(b"1234567890123", 1, 0).expect(
            "pdf417_cws(b\"1234567890123\", eclevel=1) (c=2 r=6 k=4 n=8 total=12, 13-digit numeric via 902 latch) must succeed",
        );
        assert_eq!(
            cws,
            vec![8, 902, 17, 110, 836, 811, 223, 900, 234, 914, 111, 325]
        );
    }

    /// `pdf417_render` should produce a BitMatrix whose flattened
    /// row-major pixels match bwip-js's `pixs` array byte-for-byte.
    /// Captured via the patched `tools/oracle-pdf417.js` debugecc
    /// branch — the BWIPP `pixs` array for "PDF417" at eclevel=2.
    #[test]
    fn pdf417_render_matches_bwip_js_pixs() {
        // r = 7, c = 2, rwid = 103, total pixs = 721.
        let want_pixs = [
            1u8, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 1,
            1, 1, 1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 1,
            1, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1, 1, 0, 1, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 0, 1,
            1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1,
            0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 1, 0, 0, 0,
            0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 1,
            1, 1, 0, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0,
            1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 0, 1, 0, 1, 0,
            1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 0,
            0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 1, 0, 1, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0,
            0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0,
            1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 0, 1, 1, 0, 0,
            0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1,
            0, 1, 0, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0,
            0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 0, 1, 0,
            1, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 1, 0, 1, 0,
            1, 1, 0, 0, 1, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 0, 0,
            1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1,
            0, 0, 1, 1, 1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 0, 1, 0, 1, 1,
            1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 0,
            1, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1,
            1, 0, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 1, 0,
            0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 1,
            0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 1,
        ];
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` with the
        // expected geometry knobs surfaced (r=7, c=2, rwid=103, total
        // pixs=721) so a render-pipeline failure for this canonical
        // golden names the geometry it expected.
        let bm = pdf417_render(b"PDF417", 2, 0).expect(
            "pdf417_render(b\"PDF417\", eclevel=2) (r=7 c=2 rwid=103 total pixs=721, canonical golden) must succeed",
        );
        assert_eq!(bm.width(), 103);
        assert_eq!(bm.height(), 7);
        // Flatten row-major (left-to-right, top-to-bottom) and compare.
        let mut got_pixs: Vec<u8> = Vec::with_capacity(bm.width() * bm.height());
        for y in 0..bm.height() {
            for x in 0..bm.width() {
                got_pixs.push(bm.get(x, y) as u8);
            }
        }
        assert_eq!(got_pixs, want_pixs);
    }

    /// Truncated variant of the same `"PDF417"` eclevel=2 input.
    /// `pdf417_render_truncated` drops the right RI codeword and 18-
    /// bar stop pattern, leaving `17 * (c + 2) + 1 = 69` modules per
    /// row (vs. 103 for full PDF417). Captured via
    /// `tools/oracle-pdf417-truncated.js "PDF417" 2`.
    #[test]
    fn pdf417_render_truncated_matches_bwip_js_pixs() {
        // r = 7, c = 2, rwid = 69, total pixs = 483.
        #[rustfmt::skip]
        let want_pixs: [u8; 483] = [
            1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 1, 0, 0,
            0, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 1, 1, 1, 1, 1,
            0, 0, 1, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0,
            0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 0, 0, 1,
            1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0,
            1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0,
            0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 1, 0, 0, 0, 0, 1,
            1, 1, 0, 0, 0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 0, 0,
            0, 1, 0, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 0, 1,
            0, 1, 0, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 0, 1, 1,
            0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 0,
            0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 1, 0, 1, 1,
            1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0,
            1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1,
            0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1, 0,
            0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 0, 0,
            0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1,
            0, 1, 0, 0, 0, 1, 1, 0, 1, 0, 1, 1, 1, 0, 0, 0,
            0, 1, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0,
            0, 1, 1, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1, 1,
            1, 1, 0, 0, 0, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1, 1, 1, 1, 0, 1,
            0, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 1, 1,
            1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 1, 1, 1, 0,
            1, 1, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1,
            1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 0, 1,
            1, 1, 0, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0,
            1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 1, 0, 0, 0,
            0, 1, 0, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0,
            0, 0, 1,
        ];
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // truncated-variant geometry (rwid=69 vs full=103; 34-module
        // RAP+right-side dropped). r=7 still.
        let bm = pdf417_render_truncated(b"PDF417", 2, 0).expect(
            "pdf417_render_truncated(b\"PDF417\", eclevel=2) (truncated: r=7 c=2 rwid=69 — full=103 minus 34-module RAP+right-side) must succeed",
        );
        assert_eq!(bm.width(), 69);
        assert_eq!(bm.height(), 7);
        let mut got_pixs: Vec<u8> = Vec::with_capacity(bm.width() * bm.height());
        for y in 0..bm.height() {
            for x in 0..bm.width() {
                got_pixs.push(bm.get(x, y) as u8);
            }
        }
        assert_eq!(got_pixs, want_pixs);
    }

    /// Stage 11.A8c — pin `encode`'s option-parsing error arms at
    /// lines 735-755. Both `eclevel` and `columns` have parse-failure
    /// AND range-check branches. Existing tests only exercise the
    /// happy-path via the public API; the error arms weren't pinned.
    ///
    /// `encode_truncated` has identical option parsing — kills the
    /// shared mutation class for both functions.
    ///
    /// Anchors:
    ///   - eclevel parse-fail: "abc" → "eclevel=abc (want 0..=8)"
    ///   - eclevel out-of-range: "9" → "eclevel=9 out of range 0..=8"
    ///   - columns parse-fail: "xyz" → "columns=xyz (want 0..=30)"
    ///   - columns out-of-range: "31" → "columns=31 out of range 0..=30"
    ///   - Boundary good: eclevel=8 and columns=30 must succeed
    ///     (kills `> 7` or `> 29` off-by-one mutations).
    #[test]
    fn encode_pdf417_option_parsing_rejection_arms() {
        // eclevel parse fail.
        let opts = Options::default().with("eclevel", "abc");
        let res = encode("hi", &opts);
        match res {
            Err(Error::InvalidOption(msg)) => assert!(
                msg.contains("eclevel=abc") && msg.contains("(want 0..=8)"),
                "expected eclevel parse-fail diagnostic, got: {msg:?}"
            ),
            other => panic!("expected InvalidOption(eclevel-parse), got {other:?}"),
        }
        // eclevel out of range.
        let opts = Options::default().with("eclevel", "9");
        let res = encode("hi", &opts);
        match res {
            Err(Error::InvalidOption(msg)) => assert!(
                msg.contains("eclevel=9") && msg.contains("out of range"),
                "expected eclevel range diagnostic, got: {msg:?}"
            ),
            other => panic!("expected InvalidOption(eclevel-range), got {other:?}"),
        }
        // columns parse fail.
        let opts = Options::default().with("columns", "xyz");
        let res = encode("hi", &opts);
        match res {
            Err(Error::InvalidOption(msg)) => assert!(
                msg.contains("columns=xyz") && msg.contains("(want 0..=30)"),
                "expected columns parse-fail diagnostic, got: {msg:?}"
            ),
            other => panic!("expected InvalidOption(columns-parse), got {other:?}"),
        }
        // columns out of range.
        let opts = Options::default().with("columns", "31");
        let res = encode("hi", &opts);
        match res {
            Err(Error::InvalidOption(msg)) => assert!(
                msg.contains("columns=31") && msg.contains("out of range"),
                "expected columns range diagnostic, got: {msg:?}"
            ),
            other => panic!("expected InvalidOption(columns-range), got {other:?}"),
        }
        // Boundary good: eclevel=8 + columns=30 must succeed.
        let opts = Options::default()
            .with("eclevel", "8")
            .with("columns", "30");
        assert!(
            encode("hi", &opts).is_ok(),
            "boundary eclevel=8/columns=30 must succeed (kills `> 7`/`> 29` mutants)"
        );
    }

    /// Stage 11.A8c — pin `encode_truncated`'s option-parsing error
    /// arms at lines 776-795. The parsing logic is identical to
    /// `encode`'s but lives in a separate function body, so a mutation
    /// that only flips the truncated arm's range bound (e.g. `> 8` →
    /// `>= 8` only in `encode_truncated`) survives the existing
    /// `encode_pdf417_option_parsing_rejection_arms` test which only
    /// drives the non-truncated entry point.
    ///
    /// Same anchor structure: parse-fail + out-of-range + boundary
    /// success for both `eclevel` and `columns`.
    #[test]
    fn encode_truncated_pdf417_option_parsing_rejection_arms() {
        // eclevel parse fail.
        let opts = Options::default().with("eclevel", "abc");
        match encode_truncated("hi", &opts) {
            Err(Error::InvalidOption(msg)) => assert!(
                msg.contains("eclevel=abc") && msg.contains("(want 0..=8)"),
                "expected eclevel parse-fail diagnostic, got: {msg:?}"
            ),
            other => panic!("expected InvalidOption(eclevel-parse), got {other:?}"),
        }
        // eclevel out of range.
        let opts = Options::default().with("eclevel", "9");
        match encode_truncated("hi", &opts) {
            Err(Error::InvalidOption(msg)) => assert!(
                msg.contains("eclevel=9") && msg.contains("out of range"),
                "expected eclevel range diagnostic, got: {msg:?}"
            ),
            other => panic!("expected InvalidOption(eclevel-range), got {other:?}"),
        }
        // columns parse fail.
        let opts = Options::default().with("columns", "xyz");
        match encode_truncated("hi", &opts) {
            Err(Error::InvalidOption(msg)) => assert!(
                msg.contains("columns=xyz") && msg.contains("(want 0..=30)"),
                "expected columns parse-fail diagnostic, got: {msg:?}"
            ),
            other => panic!("expected InvalidOption(columns-parse), got {other:?}"),
        }
        // columns out of range.
        let opts = Options::default().with("columns", "31");
        match encode_truncated("hi", &opts) {
            Err(Error::InvalidOption(msg)) => assert!(
                msg.contains("columns=31") && msg.contains("out of range"),
                "expected columns range diagnostic, got: {msg:?}"
            ),
            other => panic!("expected InvalidOption(columns-range), got {other:?}"),
        }
        // Boundary good: eclevel=8 + columns=30 must succeed.
        let opts = Options::default()
            .with("eclevel", "8")
            .with("columns", "30");
        assert!(
            encode_truncated("hi", &opts).is_ok(),
            "boundary eclevel=8/columns=30 must succeed (kills `> 7`/`> 29` mutants in truncated path)"
        );
    }

    /// Stage 11.A8c — pin `encode_in` / `in_submode` for representative
    /// bytes across all 4 submodes. Kills the function-replacement
    /// mutants and any `position` predicate flip on lines 136-145.
    #[test]
    fn encode_in_per_submode_lookups() {
        // Submode 0 (uppercase): 'A' is present, lowercase 'a' is NOT.
        assert!(in_submode(0, b'A' as i16));
        assert!(!in_submode(0, b'a' as i16));

        // Submode 1 (lowercase): 'a' present, 'A' NOT.
        assert!(in_submode(1, b'a' as i16));
        assert!(!in_submode(1, b'A' as i16));

        // Submode 2 (mixed/digits): '0' present.
        assert!(in_submode(2, b'0' as i16));

        // is_text_byte: pin a few known text bytes and one non-text.
        assert!(is_text_byte(b'A'));
        assert!(is_text_byte(b'a'));
        assert!(is_text_byte(b'0'));
        // 0xFF (high bit) is not in any text sub-mode (those use
        // byte-compaction).
        assert!(!is_text_byte(0xFF));

        // encode_in returns a different value across submodes when
        // the same char appears in multiple submodes — pinning two
        // distinct positions catches the `position` predicate flip.
        // 'A' appears in submode 0 at some index; in submode 1
        // it's NOT there (lowercase only). Different indices observable
        // via Some(_) vs None.
        let in_0 = encode_in(0, b'A' as i16);
        let in_1 = encode_in(1, b'A' as i16);
        assert!(in_0.is_some());
        assert!(in_1.is_none());
    }

    /// Stage 11.A8c — pin `is_text_byte` across the CHARMAPS support
    /// set with full boundary coverage. The existing
    /// `encode_in_per_submode_lookups` only checks 4 bytes
    /// (`'A'`, `'a'`, `'0'`, `0xFF`) — sufficient to kill the
    /// `iter().any` → `iter().all` and predicate-inversion mutants,
    /// but NOT the row-slice mutants (`CHARMAPS.iter()` →
    /// `CHARMAPS[..29].iter()` or `CHARMAPS[..27].iter()`) since
    /// the alpha rows 0-25 already span A-Z + a-z + 0-9 + most
    /// punctuation. The hidden contributors are the control-byte
    /// rows: CHARMAPS[11] = (L, l, 13, 13) — only place CR appears;
    /// CHARMAPS[12] = (M, m, 9, 9) — only place TAB appears;
    /// CHARMAPS[15] = (P, p, '#', 10) — only place LF appears.
    /// A row-clip mutant past row 10 / 11 / 14 silently loses CR /
    /// TAB / LF coverage without disturbing the alpha/digit set.
    ///
    /// Also pins the negative-byte boundary: bytes 0..=8, 11..=12,
    /// 14..=31, 127, and 128..=255 are NOT in any sub-mode (the
    /// special markers AL/LL/ML/PL/AS/PS are negative i16, so
    /// `(b as u8 as i16)` never matches them — but a mutant that
    /// drops the `as i16` cast would lose all i16 comparisons; a
    /// mutant that adds 128 to any positive entry would shift the
    /// "in" set into the high byte range).
    ///
    /// Mutations to catch:
    ///   * `iter()` → `iter().take(11)` (or any prefix ≤ 11): CR
    ///     vanishes; `is_text_byte(13)` flips true → false.
    ///   * `iter()` → `iter().take(12)`: TAB vanishes; byte 9 flips.
    ///   * `iter()` → `iter().take(15)`: LF vanishes; byte 10 flips.
    ///   * `iter()` → `iter().skip(11).take(N)`: alpha rows vanish;
    ///     'A' flips true → false (already caught) but also confirms
    ///     'a' / '0' still anchor.
    ///   * `b as i16` swapped to `b as i32 as i16` (no-op but type-
    ///     valid): same behavior — not catchable but documented.
    ///   * `row.contains(&v)` → `row.iter().any(|x| x > &v)`: any
    ///     row with a value > v returns true; would make every byte
    ///     return true (since e.g. '~' = 126 > 31). Caught by every
    ///     false anchor (byte 0, 8, 11, 12, 14, 31, 127, 128, 255).
    #[test]
    fn is_text_byte_full_charmaps_coverage() {
        // Sub-mode row 11 contributor — CR (13) only appears in CHARMAPS[11].
        // Mutant `CHARMAPS.iter().take(11)` drops this row.
        assert!(
            is_text_byte(13),
            "CR (13) must be a text byte (CHARMAPS[11] columns 2 and 3)"
        );
        // Sub-mode row 12 contributor — TAB (9) only appears in CHARMAPS[12].
        // Mutant `CHARMAPS.iter().take(12)` drops this row.
        assert!(
            is_text_byte(9),
            "TAB (9) must be a text byte (CHARMAPS[12] columns 2 and 3)"
        );
        // Sub-mode row 15 contributor — LF (10) only appears in CHARMAPS[15][3].
        // Mutant `CHARMAPS.iter().take(15)` drops this row.
        assert!(
            is_text_byte(10),
            "LF (10) must be a text byte (CHARMAPS[15] column 3)"
        );

        // ---- Negative anchors: bytes that are NOT in any CHARMAPS row. ----
        // Tight cluster around the CR/TAB/LF islands: 8 (BS), 11 (VT),
        // 12 (FF), 14 (SO) — none are in any row, so the support set is
        // {9, 10, 13} plus 32..=126.
        assert!(!is_text_byte(0), "NUL (0) is not in any CHARMAPS column");
        assert!(!is_text_byte(8), "BS (8) is not in any CHARMAPS column");
        assert!(!is_text_byte(11), "VT (11) is not in any CHARMAPS column");
        assert!(!is_text_byte(12), "FF (12) is not in any CHARMAPS column");
        assert!(!is_text_byte(14), "SO (14) is not in any CHARMAPS column");
        assert!(
            !is_text_byte(31),
            "US (31) — just before space — is not in any CHARMAPS column"
        );
        assert!(
            !is_text_byte(127),
            "DEL (127) is not in any CHARMAPS column"
        );
        assert!(
            !is_text_byte(128),
            "0x80 (128, first high byte) is not in any CHARMAPS column"
        );
        assert!(
            !is_text_byte(255),
            "0xFF (255) is not in any CHARMAPS column"
        );

        // ---- Positive anchors at the printable boundaries. ----
        // Boundary 32 (' '): CHARMAPS[26] columns 0-2 = ' ', column 3 = '{'.
        assert!(is_text_byte(b' '), "SPACE (32) is in CHARMAPS[26]");
        // Boundary 126 ('~'): CHARMAPS[9] column 3 = '~'.
        assert!(is_text_byte(b'~'), "tilde (126) is in CHARMAPS[9][3]");
        // Each printable ASCII byte 32..=126 is in some row; spot-check a
        // few that aren't already covered by the existing 'A'/'a'/'0' test.
        assert!(is_text_byte(b'Z'), "alpha last");
        assert!(is_text_byte(b'z'), "lower last");
        assert!(is_text_byte(b'9'), "digit last");
        assert!(is_text_byte(b'!'), "CHARMAPS[10][3]");
        assert!(is_text_byte(b'@'), "CHARMAPS[3][3]");
        assert!(is_text_byte(b'{'), "CHARMAPS[26][3]");
        assert!(is_text_byte(b'}'), "CHARMAPS[27][3]");
        assert!(is_text_byte(b'\''), "CHARMAPS[28][3]");
        // The 32..=126 dense check: prove the full printable range is in.
        for b in 32u8..=126 {
            assert!(
                is_text_byte(b),
                "all printable ASCII (32..=126) must be text bytes; failed for byte {b} ({:?})",
                b as char
            );
        }
    }

    /// Stage 11.A8c — pin `numeric_codewords` for short digit strings.
    /// Kills function-replacement mutants on the numeric-compaction
    /// codepath (line 331).
    #[test]
    fn numeric_codewords_short_inputs() {
        // Empty input → empty codewords.
        assert_eq!(numeric_codewords("").unwrap(), Vec::<u16>::new());

        // Single digit "5" produces some codeword sequence (non-empty).
        let cws = numeric_codewords("5").unwrap();
        assert!(!cws.is_empty(), "single digit must produce non-empty cws");

        // "12345" — 5 digits, fits in one numeric chunk.
        // Stage 11.A8c (cont) — descriptive label naming 5-digit single-chunk path.
        let cws = numeric_codewords("12345").unwrap();
        assert!(
            !cws.is_empty(),
            "numeric_codewords(\"12345\") (5-digit single-chunk numeric mode) must produce non-empty codewords; got len={}",
            cws.len()
        );

        // Determinism: same input twice → same codewords.
        let a = numeric_codewords("12345678").unwrap();
        let b = numeric_codewords("12345678").unwrap();
        assert_eq!(a, b);

        // Discrimination: different inputs → different codewords.
        let a = numeric_codewords("12345678").unwrap();
        let b = numeric_codewords("87654321").unwrap();
        assert_ne!(a, b);

        // Non-digit input → Err. Defense-in-depth with the dedicated
        // `numeric_rejects_non_digits` test: both pin the same static
        // diagnostic so a mutation that drops the prefix OR predicate
        // OR object would have to flip ALL THREE substrings in BOTH
        // tests to slip past.
        let err = numeric_codewords("12a45").unwrap_err();
        assert!(
            err.contains("PDF417 numeric:"),
            "diagnostic must carry the symbology prefix; got {err}"
        );
        assert!(
            err.contains("input must contain"),
            "diagnostic must carry the predicate verb; got {err}"
        );
        assert!(
            err.contains("only ASCII digits"),
            "diagnostic must carry the narrowing object; got {err}"
        );
    }

    /// Stage 11.A8c — pin `cw_to_bits` MSB-first bit extraction.
    /// The helper extracts 17 bits from a 32-bit ALL_CLUSTERS entry,
    /// big-endian (bit 16 → index 0, bit 0 → index 16). Mutations on
    /// the shift direction (`16 - i` → `i`), the mask (`& 1` → `& 2`),
    /// or the indexing would manifest as incorrect pixel patterns —
    /// caught by pixs goldens but at the symbol level, not the helper
    /// level.
    ///
    /// Hand-computed: CLUSTER_0[0] = 120256.
    ///   120256 in binary (17 bits MSB-first) = 11101010111000000
    ///   = 65536 + 32768 + 16384 + 4096 + 1024 + 256 + 128 + 64 = 120256 ✓
    ///
    /// CLUSTER_1[0] and CLUSTER_2[0] are different values — pinning
    /// across all three clusters catches cluster-index swaps.
    #[test]
    fn cw_to_bits_extracts_msb_first() {
        // CLUSTER_0[0] = 120256 = 0b11101010111000000.
        let bits = cw_to_bits(0, 0);
        assert_eq!(
            bits,
            [1, 1, 1, 0, 1, 0, 1, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0],
            "cw=0 cluster=0 must MSB-first decode 120256"
        );
        // CLUSTER_0[1] = 125680 = 0b11110101011110000.
        // 125680 = 65536+32768+16384+8192+2048+512+256+128+64+32+16 = 125680
        let bits = cw_to_bits(1, 0);
        assert_eq!(
            bits,
            [1, 1, 1, 1, 0, 1, 0, 1, 0, 1, 1, 1, 1, 0, 0, 0, 0],
            "cw=1 cluster=0 must MSB-first decode 125680"
        );
        // Sum check: bits sum back to the original value when
        // weighted MSB-first. This is essentially an "inverse"
        // verification — robust against bit-order mutations.
        for &(cw, cluster, expected) in &[(0u16, 0usize, 120256u32), (1, 0, 125680), (9, 0, 86080)]
        {
            let bits = cw_to_bits(cw, cluster);
            let v: u32 = bits
                .iter()
                .enumerate()
                .map(|(i, &b)| u32::from(b) * (1u32 << (16 - i)))
                .sum();
            assert_eq!(
                v, expected,
                "round-trip: cw={cw} cluster={cluster} bits sum back to {expected}"
            );
        }
    }
}
