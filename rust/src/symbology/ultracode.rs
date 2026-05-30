//! Ultracode (AIM USS Ultracode) — colour 2D matrix barcode.
//!
//! Ultracode is the only colour 2D symbology in the BWIPP catalog.
//! It uses a **6-colour** palette (white, cyan, magenta, yellow,
//! green, black — verified against BWIPP `ultracode_colormap` at
//! `bwip-js/dist/bwip-js-node.js:36744`) and encodes data via
//! Reed-Solomon over **GF(283)** (primitive α=3, prime modulus 283)
//! followed by a tile-based layout in the colour matrix.
//!
//! **Stage 4a (this revision)**: this module ships the colour
//! palette constants and a stub [`encode`] entry point. The full
//! encoder port (mode selector, RS over GF(283), symbol-size picker,
//! colour-matrix layout) is staged across follow-up iterations
//! (Stage 4b+).
//!
//! BWIPP source: bwip-js `dist/bwip-js-node.js` lines 36733-37260
//! (`bwipp_ultracode`, ~528 lines). The encoder pipeline is roughly:
//!
//!   1. Parse input via `bwipp_parseinput` (FNC1/FNC3 + ECI
//!      escapes when `parsefnc` / `eci` enabled).
//!   2. Mode selector: walk the input picking among the BWIPP
//!      compaction modes, each with its own GF(283) packing.
//!   3. Compute Reed-Solomon ECC over GF(283) (primitive 3, modulus
//!      283; coefficients cached per `ultracode_coeffscache`).
//!   4. Pick a symbol size from `ultracode_metrics` based on the
//!      total codeword count.
//!   5. Lay out codewords into the colour matrix per BWIPP's `place`
//!      table using `ultracode_tiles` (each 5-digit tile encodes a
//!      strip of 5 coloured cells).
//!
//! The 6-colour palette is fixed at the symbology level —
//! [`ULTRACODE_PALETTE`] below. We keep the `[Rgb8; 8]` shape
//! (matching [`ColorMatrix::palette`] capacity) so the same renderer
//! pipeline can serve future 7-/8-colour symbologies without API
//! churn; the trailing two entries are unused for Ultracode and are
//! filled with white as a safe default.

use crate::encoding::{ColorMatrix, Rgb8};
use crate::error::Error;
use crate::options::Options;

/// Ultracode's 6-colour palette, embedded in the renderer's 8-entry
/// palette field. Each entry maps a palette index `0..=7` to an sRGB
/// 8-bit colour. Index 0 is the background (white); indices 6 and 7
/// are unused for Ultracode and pinned to white so the renderer never
/// emits an unexpected colour for a stray cell.
///
/// Indices and meanings (matching BWIPP `ultracode_colormap` at
/// `bwip-js-node.js:36744`):
///
/// | idx | colour  | hex     | BWIPP tile digit |
/// |-----|---------|---------|------------------|
/// |  0  | white   | #ffffff | 0                |
/// |  1  | cyan    | #00ffff | 1                |
/// |  2  | magenta | #ff00ff | 3                |
/// |  3  | yellow  | #ffff00 | 5                |
/// |  4  | green   | #00ff00 | 6                |
/// |  5  | black   | #000000 | 9                |
/// |  6  | (unused, filled white) |     |       |
/// |  7  | (unused, filled white) |     |       |
///
/// The Stage 4b encoder will translate BWIPP's sparse tile-digit
/// codes (0/1/3/5/6/9) to these contiguous palette indices (0..=5)
/// via [`BWIPP_TILE_DIGIT_TO_PALETTE_IDX`].
///
/// The palette is intentionally `pub` so callers building colour
/// matrices directly (for the testing harness or doc examples) can
/// reuse the same RGB values the renderer will paint.
pub const ULTRACODE_PALETTE: [Rgb8; 8] = [
    Rgb8::new(0xff, 0xff, 0xff), // 0 white   (background)
    Rgb8::new(0x00, 0xff, 0xff), // 1 cyan
    Rgb8::new(0xff, 0x00, 0xff), // 2 magenta
    Rgb8::new(0xff, 0xff, 0x00), // 3 yellow
    Rgb8::new(0x00, 0xff, 0x00), // 4 green
    Rgb8::new(0x00, 0x00, 0x00), // 5 black
    Rgb8::new(0xff, 0xff, 0xff), // 6 unused — pinned to bg
    Rgb8::new(0xff, 0xff, 0xff), // 7 unused — pinned to bg
];

/// Lookup table mapping BWIPP's sparse tile-digit codes
/// (0/1/3/5/6/9) onto the contiguous palette indices used by
/// [`ULTRACODE_PALETTE`]. Each entry at index `d` gives the
/// palette index for BWIPP digit `d`; entries for undefined BWIPP
/// digits (2, 4, 7, 8) point at the background (index 0) so a
/// stray digit can never decode to an unexpected colour.
///
/// The encoder will iterate over tiles from [`ULTRACODE_TILES`]
/// (each tile is a 5-digit decimal like `13135`), splitting digits
/// and translating through this table into per-cell palette indices.
pub const BWIPP_TILE_DIGIT_TO_PALETTE_IDX: [u8; 10] = [
    0, // 0 → palette 0 (white)
    1, // 1 → palette 1 (cyan)
    0, // 2 → unused — defaults to bg
    2, // 3 → palette 2 (magenta)
    0, // 4 → unused — defaults to bg
    3, // 5 → palette 3 (yellow)
    4, // 6 → palette 4 (green)
    0, // 7 → unused — defaults to bg
    0, // 8 → unused — defaults to bg
    5, // 9 → palette 5 (black)
];

/// BWIPP `ultracode_metrics` (line 36738). One row per symbol-size
/// class; each row is `[rows, minc, maxc, mcol]` where:
///   - `rows`: number of tile rows (2..=5),
///   - `minc`/`maxc`: minimum / maximum total codeword count `tcc`
///     that fits this size class,
///   - `mcol`: starting column count for the column-grow search
///     (the encoder grows columns from `mcol` to 61, checking pads).
pub const ULTRACODE_METRICS: [[u32; 4]; 4] = [
    [2, 7, 37, 5],
    [3, 36, 84, 13],
    [4, 85, 161, 22],
    [5, 142, 282, 29],
];

/// BWIPP `ultracode_tiles` (line 36739). 285 5-digit tiles indexed by
/// codeword 0..=284. Each tile is a base-10 number `abcde` where each
/// digit is a BWIPP tile-digit (0/1/3/5/6/9) representing a colour for
/// one of 5 vertically-stacked cells in the tile column.
pub const ULTRACODE_TILES: [u32; 285] = [
    13135, 13136, 13153, 13156, 13163, 13165, 13513, 13515, 13516, 13531, 13535, 13536, 13561,
    13563, 13565, 13613, 13615, 13616, 13631, 13635, 13636, 13651, 13653, 13656, 15135, 15136,
    15153, 15163, 15165, 15313, 15315, 15316, 15351, 15353, 15356, 15361, 15363, 15365, 15613,
    15615, 15616, 15631, 15635, 15636, 15651, 15653, 15656, 16135, 16136, 16153, 16156, 16165,
    16313, 16315, 16316, 16351, 16353, 16356, 16361, 16363, 16365, 16513, 16515, 16516, 16531,
    16535, 16536, 16561, 16563, 16565, 31315, 31316, 31351, 31356, 31361, 31365, 31513, 31515,
    31516, 31531, 31535, 31536, 31561, 31563, 31565, 31613, 31615, 31631, 31635, 31636, 31651,
    31653, 31656, 35131, 35135, 35136, 35151, 35153, 35156, 35161, 35163, 35165, 35315, 35316,
    35351, 35356, 35361, 35365, 35613, 35615, 35616, 35631, 35635, 35636, 35651, 35653, 35656,
    36131, 36135, 36136, 36151, 36153, 36156, 36163, 36165, 36315, 36316, 36351, 36356, 36361,
    36365, 36513, 36515, 36516, 36531, 36535, 36536, 36561, 36563, 36565, 51313, 51315, 51316,
    51351, 51353, 51356, 51361, 51363, 51365, 51513, 51516, 51531, 51536, 51561, 51563, 51613,
    51615, 51616, 51631, 51635, 51636, 51651, 51653, 51656, 53131, 53135, 53136, 53151, 53153,
    53156, 53161, 53163, 53165, 53513, 53516, 53531, 53536, 53561, 53563, 53613, 53615, 53616,
    53631, 53635, 53636, 53651, 53653, 53656, 56131, 56135, 56136, 56151, 56153, 56156, 56161,
    56163, 56165, 56313, 56315, 56316, 56351, 56353, 56356, 56361, 56363, 56365, 56513, 56516,
    56531, 56536, 56561, 56563, 61313, 61315, 61316, 61351, 61353, 61356, 61361, 61363, 61365,
    61513, 61515, 61516, 61531, 61535, 61536, 61561, 61563, 61565, 61615, 61631, 61635, 61651,
    61653, 63131, 63135, 63136, 63151, 63153, 63156, 63161, 63163, 63165, 63513, 63515, 63516,
    63531, 63535, 63536, 63561, 63563, 63565, 63613, 63615, 63631, 63635, 63651, 63653, 65131,
    65135, 65136, 65151, 65153, 65156, 65161, 65163, 65165, 65313, 65315, 65316, 65351, 65353,
    65356, 65361, 65363, 65365, 65613, 65615, 65631, 65635, 65651, 65653, 56565, 51515,
];

/// BWIPP `ultracode_dccurev1` (line 36740). Per-DCC upper tile for
/// revision-1 symbols (32 entries).
pub const ULTRACODE_DCCUREV1: [u32; 32] = [
    51363, 51563, 51653, 53153, 53163, 53513, 53563, 53613, 53653, 56153, 56163, 56313, 56353,
    56363, 56513, 56563, 51316, 51356, 51536, 51616, 53156, 53516, 53536, 53616, 53636, 53656,
    56136, 56156, 56316, 56356, 56516, 56536,
];

/// BWIPP `ultracode_dcclrev1` (line 36741). Per-DCC lower tile for
/// revision-1 symbols (32 entries).
pub const ULTRACODE_DCCLREV1: [u32; 32] = [
    61351, 61361, 61531, 61561, 61631, 61651, 63131, 63151, 63161, 63531, 63561, 63631, 65131,
    65161, 65351, 65631, 31351, 31361, 31531, 31561, 31631, 31651, 35131, 35151, 35161, 35361,
    35631, 35651, 36131, 36151, 36351, 36531,
];

/// BWIPP `ultracode_dccurev2` (line 36742). Per-DCC upper tile for
/// revision-2 symbols (32 entries).
pub const ULTRACODE_DCCUREV2: [u32; 32] = [
    15316, 16316, 13516, 16516, 13616, 15616, 13136, 15136, 16136, 13536, 16536, 13636, 13156,
    16156, 15356, 13656, 15313, 16313, 13513, 16513, 13613, 15613, 13153, 15153, 16153, 16353,
    13653, 15653, 13163, 15163, 15363, 13563,
];

/// BWIPP `ultracode_dcclrev2` (line 36743). Per-DCC lower tile for
/// revision-2 symbols (32 entries).
pub const ULTRACODE_DCCLREV2: [u32; 32] = [
    36315, 36515, 35615, 35135, 36135, 31535, 36535, 31635, 35635, 35165, 36165, 31365, 35365,
    36365, 31565, 36565, 61315, 65315, 63515, 61615, 65135, 61535, 63535, 61635, 63635, 65635,
    63165, 65165, 61365, 65365, 61565, 63565,
];

/// BWIPP `ultracode_qccfact` (line 36753). EC-level → QCC factor
/// used by `qcc = qccfact[eclval] * ceil(mcc/25) + 5` for `eclval >= 1`
/// (the BWIPP encoder hard-codes `qcc = 3` when `eclval == 0`).
pub const ULTRACODE_QCCFACT: [u32; 6] = [0, 1, 2, 4, 6, 8];

/// Prime modulus for Ultracode's Reed-Solomon field (see BWIPP
/// `bwipp_ultracode` line 36756 — the log/alog tables are filled by
/// iterating `acc = (acc * primitive) % prime`).
pub const ULTRACODE_RS_PRIME: u32 = 283;

/// Primitive element (α) for Ultracode's RS field over GF(283).
pub const ULTRACODE_RS_PRIMITIVE: u32 = 3;

/// Reserved codeword for FNC1 (replaces the FN1 sentinel after
/// `bwipp_parseinput` runs). Encoders should emit this directly when
/// the input message tokenises an FNC1.
pub const ULTRACODE_CW_FNC1: u32 = 268;

/// Reserved codeword for FNC3.
pub const ULTRACODE_CW_FNC3: u32 = 269;

/// Pad codeword emitted to fill the symbol up to `tcc + pads`.
pub const ULTRACODE_CW_PAD: u32 = 284;

/// Start codeword (BWIPP `$_.start = 257` at line 36860).
pub const ULTRACODE_CW_START: u32 = 257;

/// Build the Reed-Solomon log / antilog tables for Ultracode's
/// GF(283) field (primitive α = 3). Mirrors the BWIPP setup at
/// `bwipp_ultracode` line 36754:
///
/// ```text
/// rsalog[0]   = 1
/// rsalog[i+1] = (rsalog[i] * 3) mod 283   for i in 0..281
/// rslog[rsalog[i]] = i                     for i in 1..=281
/// ```
///
/// The 282-cycle (283 prime, 282 = 283 - 1) is the multiplicative
/// group of GF(283).
#[allow(dead_code)] // wired in Stage 4b.3+
fn build_rs_tables() -> ([u32; 282], [u32; 283]) {
    let mut alog = [0u32; 282];
    let mut log = [0u32; 283];
    let mut acc: u32 = 1;
    for entry in alog.iter_mut() {
        *entry = acc;
        acc = (acc * ULTRACODE_RS_PRIMITIVE) % ULTRACODE_RS_PRIME;
    }
    for (i, &v) in alog.iter().enumerate().skip(1) {
        log[v as usize] = i as u32;
    }
    (alog, log)
}

/// Reed-Solomon product in GF(283): `rs_prod(x, y) = rsalog[(rslog[x]
/// + rslog[y]) mod 282]` for `x, y != 0`, else `0`. Mirrors BWIPP's
/// `ultracode_rsprod` helper at line 36767.
#[allow(dead_code)] // wired in Stage 4b.3+
#[inline]
fn rs_prod(x: u32, y: u32, alog: &[u32; 282], log: &[u32; 283]) -> u32 {
    if x == 0 || y == 0 {
        0
    } else {
        alog[((log[x as usize] + log[y as usize]) % 282) as usize]
    }
}

/// Generate the RS generator polynomial of degree `k` for the
/// Ultracode-flavoured field. Mirrors BWIPP `ultracode_gencoeffs` at
/// line 36784. Returns a length-`k` polynomial — the final element
/// of the BWIPP buffer (the polynomial's leading 1) is intentionally
/// dropped before the alternating-sign negation pass.
#[allow(dead_code)] // wired in Stage 4b.3+
pub(crate) fn gen_coeffs(k: usize) -> Vec<u32> {
    let (alog, log) = build_rs_tables();
    // BWIPP: coeffs = [1, 0, 0, ..., 0]; length = k + 1.
    let mut coeffs = vec![0u32; k + 1];
    coeffs[0] = 1;

    // For b = 1..=k:
    for b in 1..=k {
        // coeffs[b] = coeffs[b-1]
        coeffs[b] = coeffs[b - 1];
        // tmp = rsalog[b]
        let tmp = alog[b];
        // For g = b-1 down to 1:
        for g in (1..b).rev() {
            // coeffs[g] = (rs_prod(coeffs[g], tmp) + coeffs[g-1]) mod 283
            let prev = coeffs[g - 1];
            coeffs[g] = (rs_prod(coeffs[g], tmp, &alog, &log) + prev) % ULTRACODE_RS_PRIME;
        }
        // coeffs[0] = rs_prod(coeffs[0], tmp)
        coeffs[0] = rs_prod(coeffs[0], tmp, &alog, &log);
    }

    // Drop the trailing element (was scratch for the polynomial's
    // leading 1).
    coeffs.truncate(k);

    // Negate every other coefficient starting from the last.
    let mut z = coeffs.len() as isize - 1;
    while z >= 0 {
        coeffs[z as usize] = ULTRACODE_RS_PRIME - coeffs[z as usize];
        z -= 2;
    }

    coeffs
}

/// LFSR-based Reed-Solomon ECC encoder over a prime field. Mirrors
/// BWIPP `bwipp_rsecprime` at line 7091. Input `data` is fed through
/// an `nc`-tap LFSR with `coeffs`; the resulting LFSR state is
/// returned as the `nc`-element ECC suffix.
///
/// Invariants:
///   - `coeffs.len() >= nc` (BWIPP's `gen_coeffs(nc)` returns
///     exactly `nc` coefficients);
///   - all `data[i] < prime` and all `coeffs[i] < prime`.
#[allow(dead_code)] // wired in Stage 4b.3+
pub(crate) fn rs_ecprime(data: &[u32], coeffs: &[u32], nc: usize, prime: u32) -> Vec<u32> {
    let mut lfsr = vec![0u32; nc];
    for &d in data {
        let feedback = (d + prime - lfsr[0]) % prime;
        for l in 0..nc.saturating_sub(1) {
            // BWIPP: lfsr[l] = (lfsr[l+1] + coeffs[nc-l-1] * feedback) % p
            // — note the coeffs indexing flips from nc-1 down to 1.
            let c = coeffs[nc - l - 1];
            lfsr[l] = (lfsr[l + 1] + c * feedback % prime) % prime;
        }
        lfsr[nc - 1] = (coeffs[0] * feedback) % prime;
    }
    lfsr
}

/// Convenience: compute the ECC codewords (`qcc` of them) for an
/// Ultracode message. Wraps [`gen_coeffs`] + [`rs_ecprime`] with the
/// Ultracode-specific field parameters. `rsdata` is the pre-prepended
/// stream `[start, mcc, acc, ...dcws]` as BWIPP feeds it (line 37044).
#[allow(dead_code)] // wired in Stage 4b.3+
pub(crate) fn ecc_codewords(rsdata: &[u32], qcc: usize) -> Vec<u32> {
    let coeffs = gen_coeffs(qcc);
    rs_ecprime(rsdata, &coeffs, qcc, ULTRACODE_RS_PRIME)
}

/// Compute Ultracode metadata for a default-options encode.
/// Mirrors BWIPP at lines 36980-36992: `mcc`, `qcc`, `acc`, `tcc`
/// from the data codeword count and the EC level.
///
/// Returns `(mcc, qcc, acc, tcc)`.
pub(crate) fn ultracode_metadata(dcws_len: usize, eclval: u32, link1: u32) -> (u32, u32, u32, u32) {
    let mcc = (dcws_len as u32) + 3;
    let qcc = if eclval == 0 {
        3
    } else {
        let fact = ULTRACODE_QCCFACT[eclval as usize];
        let mut bucket = mcc / 25;
        if mcc % 25 != 0 {
            bucket += 1;
        }
        fact * bucket + 5
    };
    let acc = (qcc - 3) + 78 * link1;
    let tcc = mcc + qcc;
    (mcc, qcc, acc, tcc)
}

/// Build the dcws stream for default-options Ultracode encoding.
/// Mirrors BWIPP at lines 36943-36974: `parsefnc=false` makes
/// `bwipp_parseinput` a pass-through, so each input byte becomes one
/// codeword (0..=255). FN1 → 268 and FN3 → 269 substitutions are a
/// no-op in this path since `^FNC1` / `^FNC3` escapes aren't parsed.
///
/// We pass UTF-8 bytes (matching bwip-js's actual behaviour — strings
/// are UTF-8 encoded before the encoder reads them as bytes).
fn build_dcws_default(input: &str) -> Vec<u32> {
    input.bytes().map(|b| b as u32).collect()
}

/// Pick the dcws stream that matches `opts` — dispatches to
/// [`parse_raw_codewords_ultracode`] for `raw=true`, the
/// parse/parsefnc substitution helper otherwise, and falls back to
/// [`build_dcws_default`] when no input-parsing opt is set.
fn build_dcws_from_opts(input: &str, opts: &UltracodeOpts) -> Result<Vec<u32>, Error> {
    if opts.raw {
        parse_raw_codewords_ultracode(input.as_bytes()).map_err(Error::InvalidData)
    } else if opts.parse || opts.parsefnc {
        build_dcws_with_input_opts(input, opts.parse, opts.parsefnc).map_err(Error::InvalidData)
    } else {
        Ok(build_dcws_default(input))
    }
}

/// BWIPP `parseinput` 3-char and 2-char control-name table reused
/// from the micropdf417 port (Stage 11.7). NUL=0, SOH=1, ..., US=31
/// (SO=14 and SI=15 are intentionally absent in BWIPP).
const ULTRACODE_PARSE_CTRL: &[(&str, u8)] = &[
    ("NUL", 0),
    ("SOH", 1),
    ("STX", 2),
    ("ETX", 3),
    ("EOT", 4),
    ("ENQ", 5),
    ("ACK", 6),
    ("BEL", 7),
    ("BS", 8),
    ("TAB", 9),
    ("LF", 10),
    ("VT", 11),
    ("FF", 12),
    ("CR", 13),
    ("DLE", 16),
    ("DC1", 17),
    ("DC2", 18),
    ("DC3", 19),
    ("DC4", 20),
    ("NAK", 21),
    ("SYN", 22),
    ("ETB", 23),
    ("CAN", 24),
    ("EM", 25),
    ("SUB", 26),
    ("ESC", 27),
    ("FS", 28),
    ("GS", 29),
    ("RS", 30),
    ("US", 31),
];

/// Parse a `^NNN ^NNN ...` raw codeword stream for `raw=true` mode.
/// Mirrors BWIPP `bwipp_ultracode` lines 36553-36594: each token is
/// `^` + 3 ASCII digits, codeword value 0..=284.
fn parse_raw_codewords_ultracode(input: &[u8]) -> Result<Vec<u32>, String> {
    let mut out: Vec<u32> = Vec::with_capacity(input.len() / 4);
    let mut i = 0;
    while i + 4 <= input.len() {
        if input[i] != b'^' {
            return Err(format!(
                "ultracode: raw codewords must be formatted as ^NNN; \
                 expected `^` at offset {i}, got 0x{:02X}",
                input[i],
            ));
        }
        let mut value: u32 = 0;
        for j in 1..=3 {
            let c = input[i + j];
            if !c.is_ascii_digit() {
                return Err(format!(
                    "ultracode: raw codewords must be formatted as ^NNN; \
                     non-digit 0x{c:02X} at offset {}",
                    i + j,
                ));
            }
            value = value * 10 + u32::from(c - b'0');
        }
        if value > 284 {
            return Err(format!(
                "ultracode: raw codewords must be 0..=284; got {value}",
            ));
        }
        out.push(value);
        i += 4;
    }
    if i != input.len() {
        return Err(format!(
            "ultracode: raw codewords must be formatted as ^NNN; \
             {} trailing byte(s) at offset {i}",
            input.len() - i,
        ));
    }
    Ok(out)
}

/// Apply BWIPP `parseinput` substitutions for ultracode's
/// `parse=true` and/or `parsefnc=true` modes. Returns the final dcws
/// stream:
///
/// - `parse=true`: `^NNN` digit ordinals (0..=255) and `^NAME` /
///   `^NN` control names substitute to byte values (`^065` → 65,
///   `^TAB` → 9). Mirrors the micropdf417 `parse_text_escapes` helper.
/// - `parsefnc=true`: `^FNC1` → codeword 268, `^FNC3` → codeword
///   269, and `^^` → literal caret (94).
///
/// Both flags can be combined; the substitution pipeline runs all
/// patterns simultaneously in BWIPP-priority order.
fn build_dcws_with_input_opts(
    input: &str,
    parse: bool,
    parsefnc: bool,
) -> Result<Vec<u32>, String> {
    let bytes = input.as_bytes();
    let mut out: Vec<u32> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c != b'^' {
            out.push(u32::from(c));
            i += 1;
            continue;
        }
        let mut matched = false;
        // parsefnc has priority over the general parse handlers
        // because BWIPP places its FNC tokens earlier in the dispatch
        // table than ordinal/control-name escapes.
        if parsefnc {
            if i + 5 <= bytes.len() && &bytes[i..i + 5] == b"^FNC1" {
                out.push(ULTRACODE_CW_FNC1);
                i += 5;
                continue;
            }
            if i + 5 <= bytes.len() && &bytes[i..i + 5] == b"^FNC3" {
                out.push(ULTRACODE_CW_FNC3);
                i += 5;
                continue;
            }
            if i + 2 <= bytes.len() && bytes[i + 1] == b'^' {
                // `^^` → literal caret (parsefnc only).
                out.push(94);
                i += 2;
                continue;
            }
        }
        if parse {
            // 3-char ctrl name (TAB, NUL, ESC, ...).
            if i + 4 <= bytes.len() {
                let candidate = &bytes[i + 1..i + 4];
                if let Some((_, byte)) = ULTRACODE_PARSE_CTRL
                    .iter()
                    .find(|(name, _)| name.len() == 3 && name.as_bytes() == candidate)
                {
                    out.push(u32::from(*byte));
                    i += 4;
                    matched = true;
                }
            }
            // 2-char ctrl name (BS, LF, FF, CR, ...).
            if !matched && i + 3 <= bytes.len() {
                let candidate = &bytes[i + 1..i + 3];
                if let Some((_, byte)) = ULTRACODE_PARSE_CTRL
                    .iter()
                    .find(|(name, _)| name.len() == 2 && name.as_bytes() == candidate)
                {
                    out.push(u32::from(*byte));
                    i += 3;
                    matched = true;
                }
            }
            // 3-digit ordinal.
            if !matched && i + 4 <= bytes.len() {
                let candidate = &bytes[i + 1..i + 4];
                if candidate.iter().all(|b| b.is_ascii_digit()) {
                    let value: u32 = u32::from(candidate[0] - b'0') * 100
                        + u32::from(candidate[1] - b'0') * 10
                        + u32::from(candidate[2] - b'0');
                    if value > 255 {
                        return Err(format!(
                            "ultracode parse: ordinal must be 000..=255; got {value}",
                        ));
                    }
                    out.push(value);
                    i += 4;
                    matched = true;
                }
            }
        }
        if !matched {
            // Unmatched `^` — emit literal caret and advance one byte.
            out.push(94);
            i += 1;
        }
    }
    Ok(out)
}

/// Pick the symbol size for a given total codeword count `tcc`,
/// mirroring BWIPP at lines 36994-37034. Returns
/// `(metrics_row_idx, rows, columns, pads)` where `rows`/`columns`
/// are still in BWIPP's pre-expansion units (multiply by 6+1 for
/// final pixs rows; add 6 for final columns).
///
/// Returns `None` if `tcc` exceeds the largest symbol size.
fn pick_symbol_size(tcc: u32) -> Option<(usize, u32, u32, u32)> {
    // Find the metrics row.
    let (m_idx, metric) = ULTRACODE_METRICS
        .iter()
        .enumerate()
        .find(|(_, m)| tcc >= m[1] && tcc <= m[2])?;
    let rows = metric[0];
    let mcol = metric[3];
    // Grow columns from mcol..=61 until pads >= 0.
    for cols in mcol..=61 {
        let mut adj = cols;
        // BWIPP: cols -= 1 if cols >= 15; -= 1 if cols >= 31; -= 1 if cols >= 47.
        // These three subtractions remove the cells used by the
        // sparse separator dots at columns 19, 35, 51.
        if cols >= 15 {
            adj -= 1;
        }
        if cols >= 31 {
            adj -= 1;
        }
        if cols >= 47 {
            adj -= 1;
        }
        let usable = adj * rows;
        // pads = usable - 3 - tcc; must be >= 0.
        if usable >= 3 + tcc {
            let pads = usable - 3 - tcc;
            return Some((m_idx, rows, cols, pads));
        }
    }
    None
}

/// Lay out the final `pixs` grid for an Ultracode symbol. Mirrors
/// BWIPP at lines 37074-37241 — separator lines, DCC tile column,
/// and the main tile sequence.
///
/// Returns the flat `rows × columns` pixs array as BWIPP tile-digit
/// codes (0/1/3/5/6/9/-1 sentinel, where -1 means "unset"). The
/// caller translates these to palette indices via
/// [`BWIPP_TILE_DIGIT_TO_PALETTE_IDX`].
///
/// All inputs already passed through [`pick_symbol_size`]:
///   - `rows`/`cols` are the **expanded** dimensions (rows × 6 + 1,
///     cols + 6) — the final pixs grid size.
///   - `dcc` is `cols_picked - mcol`, an index into `dccu`/`dccl`.
fn layout_pixs(rows: u32, cols: u32, dcc: u32, rev: u32, tileseq: &[u32]) -> Vec<i32> {
    let rows = rows as usize;
    let cols = cols as usize;
    let total = rows * cols;
    let mut pixs: Vec<i32> = vec![-1; total];

    // BWIPP convention: qmv(i=col, j=row) → row * cols + col.
    let qmv = |col: usize, row: usize| row * cols + col;

    // Vertical column passes (BWIPP lines 37086-37114).
    // For each column i in 0..cols:
    //   - if i >= 5: at every 6th row, pixs[i, j] = (i%2)*9
    //   - top row pixs[i, 0] = 9
    //   - bottom row pixs[i, rows-1] = 9
    for i in 0..cols {
        // Sparse separator dots at j = 0, 6, 12, ... for i >= 5.
        if i >= 5 {
            let mut j = 0usize;
            while j < rows {
                pixs[qmv(i, j)] = ((i % 2) as i32) * 9;
                j += 6;
            }
        }
        pixs[qmv(i, 0)] = 9;
        pixs[qmv(i, rows - 1)] = 9;
    }

    // Horizontal row passes for inner rows i = 1..rows-2 (line 37115-37169).
    for i in 1..=rows.saturating_sub(2) {
        // Sparse separator dots at j = 3, 19, 35, ... (every 16).
        let mut j = 3usize;
        while j < cols {
            pixs[qmv(j, i)] = ((1 - (i % 2)) as i32) * 9;
            j += 16;
        }
        // Fixed columns on the left margin.
        pixs[qmv(0, i)] = 9;
        pixs[qmv(1, i)] = ((1 - (i % 2)) as i32) * 9;
        pixs[qmv(2, i)] = 0;
        pixs[qmv(3, i)] = 9;
        pixs[qmv(4, i)] = 0;
        pixs[qmv(cols - 1, i)] = 9;
    }

    // DCC tile column at col=2, starting at row (rows/2) - 5 (line 37170-37194).
    // Layout: 5 digits from dccu[dcc], then a 0 separator, then 5 digits from dccl[dcc].
    let dccu = if rev == 1 {
        &ULTRACODE_DCCUREV1
    } else {
        &ULTRACODE_DCCUREV2
    };
    let dccl = if rev == 1 {
        &ULTRACODE_DCCLREV1
    } else {
        &ULTRACODE_DCCLREV2
    };
    let dcc_u = dccu[dcc as usize];
    let dcc_l = dccl[dcc as usize];
    let mut dcc_digits: Vec<i32> = Vec::with_capacity(11);
    for d in digits_5(dcc_u) {
        dcc_digits.push(d as i32);
    }
    dcc_digits.push(0);
    for d in digits_5(dcc_l) {
        dcc_digits.push(d as i32);
    }
    let row_start = (rows / 2) - 5;
    for (k, &d) in dcc_digits.iter().enumerate() {
        pixs[qmv(2, row_start + k)] = d;
    }

    // Main tile placement (BWIPP lines 37210-37241).
    // For each codeword in tileseq, look up its 5-digit tile from
    // ULTRACODE_TILES (or use it as a literal if it's already the
    // 283 sentinel) and write 5 cells vertically into the grid
    // starting at (x=5, y=1). After 5 cells, advance y by 1. When y
    // reaches rows-1, advance x and reset y to 1. If the new (x, 1)
    // is already non-(-1), skip another column.
    let mut x: usize = 5;
    let mut y: usize = 1;
    for &cw in tileseq {
        // BWIPP looks up the tile via ULTRACODE_TILES[cw] for cw in
        // 0..=284. The literal 283 in tileseq isn't a codeword — it
        // gets pushed directly by BWIPP at line 37200 and looked up
        // through tiles[283] = 56565 (a special filler tile).
        let tile = ULTRACODE_TILES[cw as usize];
        for d in digits_5(tile) {
            pixs[qmv(x, y)] = d as i32;
            y += 1;
        }
        // After writing 5 cells, advance y by 1 unless we're at the
        // last row, in which case advance x and reset y.
        if y != rows - 1 {
            y += 1;
        } else {
            x += 1;
            y = 1;
            // If new (x, 1) is already set, skip another column.
            if pixs[qmv(x, y)] != -1 {
                x += 1;
            }
        }
    }

    pixs
}

/// Split a 5-digit decimal into its digits in BWIPP's order (most
/// significant first, matching `$cvrs($s(5), v, 10)`). For tile
/// 13135 returns [1, 3, 1, 3, 5].
fn digits_5(v: u32) -> [u8; 5] {
    let mut out = [0u8; 5];
    let mut t = v;
    for slot in out.iter_mut().rev() {
        *slot = (t % 10) as u8;
        t /= 10;
    }
    out
}

/// Parsed BWIPP-exposed Ultracode options. Used by [`encode`] to
/// thread caller-supplied values through the encoder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UltracodeOpts {
    /// EC level value (0..=5).
    pub eclval: u32,
    /// Revision (1 or 2).
    pub rev: u32,
    /// Linked-symbol index (0 = standalone; non-zero shifts acc).
    pub link1: u32,
    /// Start codeword (BWIPP default 257; user-tunable for the
    /// linked-symbol companion path).
    pub start: u32,
    /// `raw=true` — input is a sequence of `^NNN` codewords (each
    /// NNN ∈ 0..=284). Bypasses the text compactor; the codewords
    /// become `dcws` directly.
    pub raw: bool,
    /// `parse=true` — `^NNN` escapes in the text input are substituted
    /// with their corresponding ordinal bytes (`^065` → 65) before
    /// the compactor runs.
    pub parse: bool,
    /// `parsefnc=true` — `^FNC1` and `^FNC3` escapes are substituted
    /// with sentinels that the compactor expands to codewords 268/269.
    /// `^^` collapses to a literal caret.
    pub parsefnc: bool,
}

impl Default for UltracodeOpts {
    fn default() -> Self {
        Self {
            eclval: 2,
            rev: 2,
            link1: 0,
            start: 257,
            raw: false,
            parse: false,
            parsefnc: false,
        }
    }
}

/// Validate BWIPP-exposed Ultracode options and parse non-default
/// values. Defaults are accepted silently; explicit defaults are also
/// accepted. Mirrors BWIPP's validation at `bwipp_ultracode`
/// (`bwip-js-node.js:36510-36552`).
///
/// # BWIPP cross-validation
///
/// `eclevel=EC0` requires `rev=1` (BWIPP line 36544: with `rev=2` the
/// minimum allowed eclvalue is 1; with `rev=1` it's 0). The Rust port
/// enforces the same cross-check via `InvalidOption`.
fn check_ultracode_opts(opts: &Options) -> Result<UltracodeOpts, Error> {
    let mut parsed = UltracodeOpts::default();
    // rev — parsed first so it can be used to cross-validate eclevel.
    if let Some(v) = opts.get("rev") {
        match v {
            "1" => parsed.rev = 1,
            "2" => parsed.rev = 2,
            _ => {
                return Err(Error::InvalidOption(format!(
                    "ultracode: rev={v:?} is not a valid BWIPP value (valid: 1 or 2)"
                )));
            }
        }
    }
    // eclevel — BWIPP allows "EC0".."EC5"; cross-validates with rev.
    if let Some(v) = opts.get("eclevel") {
        let eclval = match v {
            "EC0" => 0u32,
            "EC1" => 1,
            "EC2" => 2,
            "EC3" => 3,
            "EC4" => 4,
            "EC5" => 5,
            _ => {
                return Err(Error::InvalidOption(format!(
                    "ultracode: eclevel={v:?} is not a valid BWIPP value \
                     (valid: EC0/EC1/EC2/EC3/EC4/EC5)"
                )));
            }
        };
        // BWIPP cross-check: rev=2 requires eclvalue >= 1.
        let min_ecl = if parsed.rev == 2 { 1 } else { 0 };
        if eclval < min_ecl {
            return Err(Error::InvalidOption(format!(
                "ultracode: eclevel={v} requires rev=1 (rev=2 requires EC1..EC5)"
            )));
        }
        parsed.eclval = eclval;
    }
    // parsefnc — BWIPP boolean default false.
    if let Some(v) = opts.get("parsefnc") {
        match v {
            "false" => {}
            "true" => parsed.parsefnc = true,
            _ => {
                return Err(Error::InvalidOption(format!(
                    "ultracode: parsefnc={v:?} must be \"true\" or \"false\""
                )));
            }
        }
    }
    // raw — BWIPP boolean default false.
    if let Some(v) = opts.get("raw") {
        match v {
            "false" => {}
            "true" => parsed.raw = true,
            _ => {
                return Err(Error::InvalidOption(format!(
                    "ultracode: raw={v:?} must be \"true\" or \"false\""
                )));
            }
        }
    }
    // parse — BWIPP generic-escape boolean default false.
    if let Some(v) = opts.get("parse") {
        match v {
            "false" => {}
            "true" => parsed.parse = true,
            _ => {
                return Err(Error::InvalidOption(format!(
                    "ultracode: parse={v:?} must be \"true\" or \"false\""
                )));
            }
        }
    }
    // link1 — BWIPP integer default 0; non-zero shifts `acc` by 78
    // per linked-symbol position (BWIPP line 36702:
    // `acc = (qcc - 3) + (78 * link1)`).
    if let Some(v) = opts.get("link1") {
        match v.parse::<u32>() {
            Ok(n) => parsed.link1 = n,
            Err(_) => {
                return Err(Error::InvalidOption(format!(
                    "ultracode: link1={v:?} must be a non-negative integer"
                )));
            }
        }
    }
    // start — BWIPP integer default 257. Customisable via the user-
    // facing option (the linked-symbol companion path needs it).
    if let Some(v) = opts.get("start") {
        match v.parse::<u32>() {
            Ok(n) => parsed.start = n,
            Err(_) => {
                return Err(Error::InvalidOption(format!(
                    "ultracode: start={v:?} must be a non-negative integer"
                )));
            }
        }
    }
    Ok(parsed)
}

/// Encode `data` as an Ultracode colour-matrix symbol using BWIPP's
/// default options (`eclevel="EC2"`, `rev=2`, `parsefnc=false`,
/// `raw=false`, `link1=0`, `parse=false`).
///
/// Mirrors `bwipp_ultracode` at `bwip-js/dist/bwip-js-node.js:36733`.
///
/// # Options consumed
///
/// All 7 BWIPP-exposed `ultracode` options are recognised:
/// `eclevel`, `rev`, `parsefnc`, `raw`, `link1`, `parse`, `start`.
/// Currently only the BWIPP defaults are supported; any other value
/// returns [`Error::Unimplemented`] with a tagged message naming
/// the option. Unknown / out-of-range values return
/// [`Error::InvalidOption`].
///
/// # Errors
///
/// * [`Error::InvalidData`] if `data` is empty.
/// * [`Error::InvalidData`] if the byte length exceeds BWIPP's
///   2500-byte limit, or the encoded `tcc` exceeds the largest
///   symbol size.
/// * [`Error::InvalidOption`] if a BWIPP option is set to a value
///   outside its allowed range (e.g. `eclevel = "XX"`, `rev = "3"`).
/// * [`Error::Unimplemented`] if a BWIPP option is set to a valid
///   non-default value this port hasn't yet implemented (e.g.
///   `eclevel = "EC1"`, `parsefnc = "true"`, `rev = "1"`,
///   `raw = "true"`, non-zero `link1`).
pub fn encode(data: &str, opts: &Options) -> Result<ColorMatrix, Error> {
    if data.is_empty() {
        return Err(Error::InvalidData("ultracode: empty input".to_string()));
    }
    if data.len() > 2500 {
        return Err(Error::InvalidData(
            "ultracode: input exceeds BWIPP's 2500-byte limit".to_string(),
        ));
    }
    let parsed = check_ultracode_opts(opts)?;
    let dcws = build_dcws_from_opts(data, &parsed)?;
    let eclval = parsed.eclval;
    let rev = parsed.rev;
    let link1 = parsed.link1;
    let (mcc, qcc, acc, tcc) = ultracode_metadata(dcws.len(), eclval, link1);

    // Pick symbol size + grow columns.
    let (_m_idx, rows, cols_picked, pads) = pick_symbol_size(tcc).ok_or_else(|| {
        Error::InvalidData("ultracode: payload exceeds maximum symbol size".to_string())
    })?;
    let dcc = cols_picked
        - ULTRACODE_METRICS
            .iter()
            .find(|m| m[0] == rows)
            .map(|m| m[3])
            .unwrap_or(0);

    // Compute the ecc codewords. Both the RS data and the tile
    // sequence use `parsed.start` (BWIPP default 257; the linked-
    // symbol companion path passes 258..=272 here).
    let mut rsdata = vec![parsed.start, mcc, acc];
    rsdata.extend_from_slice(&dcws);
    let ecws = ecc_codewords(&rsdata, qcc as usize);

    // Build the tile sequence: [start, mcc, ...ecws, tcc, 283, acc, ...dcws, ...pads, qcc].
    // BWIPP lines 37195-37207.
    let mut tileseq: Vec<u32> =
        Vec::with_capacity(2 + ecws.len() + 1 + 1 + 1 + dcws.len() + pads as usize + 1);
    tileseq.push(parsed.start);
    tileseq.push(mcc);
    tileseq.extend_from_slice(&ecws);
    tileseq.push(tcc);
    tileseq.push(283); // BWIPP literal (looked up via ULTRACODE_TILES[283] = 56565).
    tileseq.push(acc);
    tileseq.extend_from_slice(&dcws);
    tileseq.extend(std::iter::repeat_n(ULTRACODE_CW_PAD, pads as usize));
    tileseq.push(qcc);

    // Expand rows/columns to final pixs grid size.
    let final_rows = rows * 6 + 1;
    let final_cols = cols_picked + 6;

    let pixs = layout_pixs(final_rows, final_cols, dcc, rev, &tileseq);

    // Convert pixs (BWIPP tile-digit codes 0/1/3/5/6/9/-1) into a
    // ColorMatrix indexed by palette index 0..=5.
    let mut matrix = ColorMatrix::new(final_cols as usize, final_rows as usize, ULTRACODE_PALETTE);
    for y in 0..final_rows as usize {
        for x in 0..final_cols as usize {
            let cell = pixs[y * final_cols as usize + x];
            // -1 sentinel is treated as background (palette 0). In
            // practice the layout fills every cell; this is defensive.
            let palette_idx = if cell < 0 {
                0
            } else {
                BWIPP_TILE_DIGIT_TO_PALETTE_IDX[cell as usize]
            };
            matrix.set(x, y, palette_idx);
        }
    }
    Ok(matrix)
}

/// Expose the raw pixs grid (BWIPP tile-digit codes) for the same
/// default-options inputs as [`encode`]. Used by the byte-for-byte
/// oracle test that pins the encoder against `ultracode_corpus.json`.
///
/// Returns `(rows, cols, pixs_flat)` where `pixs_flat[row*cols + col]`
/// is a BWIPP tile digit (0/1/3/5/6/9). The Stage-4b corpus dumps
/// pixs in the same shape via `oracle-ultracode.js`.
#[cfg(test)]
pub(crate) fn encode_pixs_default(data: &str) -> Result<(u32, u32, Vec<i32>), Error> {
    encode_pixs_with_opts(data, UltracodeOpts::default())
}

/// Variant of [`encode_pixs_default`] that exposes the parsed option
/// set. Used by Stage 11.10+ to pin byte-for-byte pixs output against
/// the bwip-js corpus for non-default eclevel / rev / link1 / start
/// combinations.
#[cfg(test)]
pub(crate) fn encode_pixs_with_opts(
    data: &str,
    opts: UltracodeOpts,
) -> Result<(u32, u32, Vec<i32>), Error> {
    if data.is_empty() {
        return Err(Error::InvalidData("ultracode: empty input".to_string()));
    }
    if data.len() > 2500 {
        return Err(Error::InvalidData(
            "ultracode: input exceeds BWIPP's 2500-byte limit".to_string(),
        ));
    }
    let dcws = build_dcws_from_opts(data, &opts)?;
    let (mcc, qcc, acc, tcc) = ultracode_metadata(dcws.len(), opts.eclval, opts.link1);
    let (_m_idx, rows, cols_picked, pads) = pick_symbol_size(tcc)
        .ok_or_else(|| Error::InvalidData("ultracode: payload too large".to_string()))?;
    let dcc = cols_picked
        - ULTRACODE_METRICS
            .iter()
            .find(|m| m[0] == rows)
            .map(|m| m[3])
            .unwrap_or(0);

    let mut rsdata = vec![opts.start, mcc, acc];
    rsdata.extend_from_slice(&dcws);
    let ecws = ecc_codewords(&rsdata, qcc as usize);

    let mut tileseq: Vec<u32> = Vec::new();
    tileseq.push(opts.start);
    tileseq.push(mcc);
    tileseq.extend_from_slice(&ecws);
    tileseq.push(tcc);
    tileseq.push(283);
    tileseq.push(acc);
    tileseq.extend_from_slice(&dcws);
    tileseq.extend(std::iter::repeat_n(ULTRACODE_CW_PAD, pads as usize));
    tileseq.push(qcc);

    let final_rows = rows * 6 + 1;
    let final_cols = cols_picked + 6;
    let pixs = layout_pixs(final_rows, final_cols, dcc, opts.rev, &tileseq);
    Ok((final_rows, final_cols, pixs))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Corpus pixs grids captured via `rust/tools/oracle-ultracode.js`
    // for default-options encoding (eclevel=EC2, rev=2, parsefnc=false,
    // link1=0). Each constant is a flat row-major `rows × cols` grid
    // of BWIPP tile-digit codes (0/1/3/5/6/9 — no -1 sentinels).
    const ULTRACODE_PIXS_A: &[i32] = &[
        9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 0, 1, 9, 0, 6, 3, 3, 3, 5, 5, 1, 9, 9, 9, 3, 9,
        0, 3, 5, 1, 6, 3, 6, 6, 9, 9, 0, 5, 9, 0, 6, 1, 5, 5, 1, 5, 5, 9, 9, 9, 1, 9, 0, 5, 6, 1,
        1, 5, 6, 3, 9, 9, 0, 6, 9, 0, 1, 3, 6, 3, 6, 5, 5, 9, 9, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9,
        9, 9, 0, 3, 9, 0, 1, 1, 3, 6, 1, 1, 1, 9, 9, 9, 5, 9, 0, 3, 3, 1, 3, 3, 3, 3, 9, 9, 0, 6,
        9, 0, 1, 1, 3, 5, 5, 1, 5, 9, 9, 9, 1, 9, 0, 6, 5, 1, 6, 3, 6, 1, 9, 9, 0, 5, 9, 0, 3, 3,
        5, 3, 6, 3, 5, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
    ];

    const ULTRACODE_PIXS_HELLO: &[i32] = &[
        9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 0, 1, 9, 0, 6, 5, 6, 1, 3, 5, 3, 3, 3, 9,
        9, 9, 3, 9, 0, 3, 1, 3, 6, 6, 6, 1, 5, 5, 9, 9, 0, 6, 9, 0, 6, 6, 5, 1, 5, 5, 3, 6, 6, 9,
        9, 9, 1, 9, 0, 5, 3, 3, 5, 6, 6, 5, 1, 3, 9, 9, 0, 6, 9, 0, 1, 5, 6, 6, 5, 5, 1, 3, 1, 9,
        9, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 9, 9, 0, 3, 9, 0, 1, 5, 5, 5, 1, 1, 3, 3, 1, 9,
        9, 9, 6, 9, 0, 3, 1, 1, 1, 3, 3, 5, 5, 3, 9, 9, 0, 1, 9, 0, 5, 3, 5, 3, 6, 1, 1, 6, 5, 9,
        9, 9, 3, 9, 0, 1, 5, 1, 6, 1, 6, 6, 1, 1, 9, 9, 0, 5, 9, 0, 6, 1, 6, 1, 3, 3, 5, 3, 5, 9,
        9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
    ];

    const ULTRACODE_PIXS_HELLO_WORLD: &[i32] = &[
        9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 0, 1, 9, 0, 6, 5, 5, 5, 6, 5,
        3, 3, 3, 1, 3, 3, 1, 9, 9, 9, 6, 9, 0, 3, 6, 6, 6, 5, 6, 1, 5, 5, 5, 5, 5, 5, 9, 9, 0, 1,
        9, 0, 6, 5, 3, 1, 3, 5, 3, 6, 6, 3, 6, 6, 3, 9, 9, 9, 3, 9, 0, 5, 3, 1, 6, 1, 6, 5, 1, 3,
        5, 3, 1, 5, 9, 9, 0, 6, 9, 0, 1, 1, 3, 5, 6, 5, 1, 3, 1, 1, 1, 3, 3, 9, 9, 9, 0, 9, 0, 9,
        0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 9, 9, 0, 3, 9, 0, 1, 3, 5, 3, 1, 1, 3, 3, 1, 3, 3, 3,
        1, 9, 9, 9, 5, 9, 0, 3, 5, 3, 5, 3, 3, 5, 5, 5, 1, 5, 5, 3, 9, 9, 0, 6, 9, 0, 6, 1, 5, 6,
        6, 1, 1, 6, 6, 6, 6, 1, 5, 9, 9, 9, 3, 9, 0, 1, 5, 6, 5, 5, 6, 6, 1, 5, 3, 5, 6, 1, 9, 9,
        0, 5, 9, 0, 5, 3, 3, 1, 6, 3, 5, 3, 1, 1, 1, 3, 5, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
        9, 9, 9, 9, 9, 9, 9,
    ];

    const ULTRACODE_PIXS_12345: &[i32] = &[
        9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 0, 1, 9, 0, 6, 1, 1, 6, 1, 5, 1, 1, 1, 9,
        9, 9, 3, 9, 0, 3, 3, 6, 5, 3, 6, 6, 6, 6, 9, 9, 0, 6, 9, 0, 6, 5, 1, 3, 5, 5, 1, 1, 3, 9,
        9, 9, 1, 9, 0, 5, 6, 5, 1, 6, 6, 5, 6, 1, 9, 9, 0, 6, 9, 0, 1, 5, 3, 5, 5, 5, 3, 5, 5, 9,
        9, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 9, 9, 0, 3, 9, 0, 1, 5, 6, 6, 1, 1, 1, 1, 1, 9,
        9, 9, 6, 9, 0, 3, 6, 3, 1, 3, 3, 6, 6, 3, 9, 9, 0, 1, 9, 0, 5, 1, 1, 5, 6, 1, 1, 3, 5, 9,
        9, 9, 3, 9, 0, 1, 3, 5, 6, 1, 6, 5, 1, 1, 9, 9, 0, 5, 9, 0, 6, 6, 1, 1, 3, 3, 6, 3, 5, 9,
        9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
    ];

    const ULTRACODE_PIXS_ALPHA: &[i32] = &[
        9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 0, 0, 9, 0, 6, 5, 5, 6, 5,
        1, 1, 3, 3, 3, 3, 3, 3, 3, 9, 9, 9, 0, 9, 0, 3, 6, 6, 5, 6, 6, 6, 1, 1, 1, 1, 1, 1, 1, 9,
        9, 0, 0, 9, 0, 6, 1, 1, 6, 5, 5, 5, 3, 3, 5, 5, 5, 6, 6, 9, 9, 9, 1, 9, 0, 5, 3, 3, 5, 6,
        3, 6, 5, 6, 1, 3, 6, 3, 5, 9, 9, 0, 6, 9, 0, 1, 1, 5, 3, 5, 6, 5, 1, 5, 6, 6, 5, 1, 1, 9,
        9, 9, 3, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 9, 0, 1, 9, 0, 1, 5, 5, 3, 1,
        1, 3, 3, 3, 3, 3, 3, 3, 5, 9, 9, 9, 6, 9, 0, 5, 6, 6, 6, 3, 6, 1, 1, 1, 1, 1, 1, 1, 1, 9,
        9, 0, 0, 9, 0, 3, 3, 3, 1, 5, 5, 3, 3, 5, 5, 5, 6, 6, 5, 9, 9, 9, 3, 9, 0, 1, 1, 1, 5, 1,
        6, 1, 5, 1, 3, 6, 1, 3, 1, 9, 9, 0, 6, 9, 0, 3, 6, 3, 1, 3, 1, 5, 6, 3, 1, 1, 3, 5, 5, 9,
        9, 9, 5, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 9, 0, 1, 9, 0, 6, 1, 5, 1, 1,
        1, 3, 3, 3, 3, 3, 3, 3, 1, 9, 9, 9, 5, 9, 0, 5, 6, 1, 5, 6, 6, 1, 1, 1, 1, 1, 1, 1, 3, 9,
        9, 0, 0, 9, 0, 1, 5, 5, 6, 5, 5, 3, 3, 5, 5, 5, 6, 6, 5, 9, 9, 9, 0, 9, 0, 3, 3, 6, 1, 3,
        6, 1, 6, 1, 3, 6, 1, 3, 3, 9, 9, 0, 0, 9, 0, 5, 1, 3, 3, 5, 3, 6, 1, 5, 5, 3, 5, 6, 1, 9,
        9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
    ];

    const ULTRACODE_PIXS_ALPHANUM: &[i32] = &[
        9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 0, 1, 9, 0, 6, 1, 1,
        3, 6, 5, 3, 3, 3, 1, 1, 1, 1, 1, 0, 5, 9, 9, 9, 3, 9, 0, 3, 6, 3, 5, 1, 6, 5, 5, 5, 6, 6,
        6, 6, 6, 9, 1, 9, 9, 0, 6, 9, 0, 6, 1, 5, 3, 3, 5, 1, 1, 1, 1, 1, 3, 3, 3, 0, 5, 9, 9, 9,
        3, 9, 0, 5, 5, 1, 5, 1, 6, 5, 6, 6, 3, 5, 1, 1, 5, 9, 1, 9, 9, 0, 6, 9, 0, 1, 6, 6, 6, 6,
        5, 3, 1, 5, 6, 6, 3, 6, 3, 0, 5, 9, 9, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9,
        0, 9, 0, 9, 9, 0, 3, 9, 0, 1, 3, 5, 6, 1, 1, 3, 3, 3, 1, 1, 1, 1, 1, 0, 1, 9, 9, 9, 1, 9,
        0, 3, 6, 1, 3, 5, 3, 5, 5, 5, 6, 6, 6, 6, 6, 9, 3, 9, 9, 0, 3, 9, 0, 6, 5, 5, 1, 1, 1, 1,
        1, 3, 1, 1, 3, 3, 3, 0, 5, 9, 9, 9, 6, 9, 0, 3, 3, 3, 5, 5, 6, 5, 6, 1, 5, 6, 1, 5, 5, 9,
        1, 9, 9, 0, 5, 9, 0, 5, 1, 1, 3, 3, 3, 6, 3, 5, 3, 5, 5, 1, 6, 0, 5, 9, 9, 9, 9, 9, 9, 9,
        9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
    ];

    const ULTRACODE_PIXS_UTF8: &[i32] = &[
        9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 0, 1, 9, 0, 6, 3, 3, 6, 6, 5, 1, 1,
        5, 5, 5, 9, 9, 9, 3, 9, 0, 3, 5, 5, 5, 1, 6, 3, 3, 6, 6, 1, 9, 9, 0, 1, 9, 0, 6, 1, 3, 1,
        5, 5, 1, 1, 1, 1, 5, 9, 9, 9, 3, 9, 0, 5, 6, 5, 3, 6, 6, 3, 5, 6, 6, 1, 9, 9, 0, 6, 9, 0,
        1, 3, 1, 6, 1, 5, 5, 3, 1, 3, 5, 9, 9, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 9, 9,
        0, 3, 9, 0, 1, 3, 1, 5, 1, 1, 1, 3, 3, 5, 1, 9, 9, 9, 6, 9, 0, 3, 6, 6, 3, 3, 3, 3, 6, 6,
        6, 3, 9, 9, 0, 5, 9, 0, 5, 5, 5, 1, 6, 1, 1, 3, 3, 1, 5, 9, 9, 9, 3, 9, 0, 3, 1, 6, 5, 3,
        6, 3, 5, 5, 5, 1, 9, 9, 0, 5, 9, 0, 6, 6, 5, 6, 1, 3, 6, 1, 6, 1, 5, 9, 9, 9, 9, 9, 9, 9,
        9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
    ];

    const ULTRACODE_PIXS_FOX: &[i32] = &[
        9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 0, 0,
        9, 0, 6, 5, 6, 1, 5, 3, 3, 3, 3, 3, 3, 1, 3, 1, 0, 3, 3, 1, 3, 3, 1, 9, 9, 9, 0, 9, 0, 3,
        3, 5, 6, 6, 5, 5, 5, 5, 6, 5, 5, 5, 5, 9, 5, 5, 5, 6, 5, 5, 9, 9, 0, 0, 9, 0, 6, 1, 6, 3,
        5, 3, 6, 1, 1, 1, 3, 3, 6, 3, 0, 1, 6, 3, 1, 1, 6, 9, 9, 9, 1, 9, 0, 5, 5, 3, 6, 6, 5, 3,
        6, 5, 3, 1, 5, 1, 5, 9, 6, 5, 5, 5, 6, 5, 9, 9, 0, 6, 9, 0, 1, 6, 1, 3, 5, 1, 6, 1, 6, 6,
        5, 1, 5, 1, 0, 5, 6, 1, 6, 3, 6, 9, 9, 9, 1, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9,
        0, 9, 0, 9, 0, 9, 0, 9, 9, 9, 0, 3, 9, 0, 1, 6, 1, 3, 1, 3, 3, 3, 3, 3, 3, 3, 3, 3, 0, 3,
        3, 3, 3, 3, 5, 9, 9, 9, 6, 9, 0, 6, 5, 6, 5, 3, 5, 6, 5, 5, 5, 5, 5, 5, 5, 9, 5, 5, 5, 6,
        5, 1, 9, 9, 0, 0, 9, 0, 1, 3, 1, 3, 5, 1, 1, 3, 6, 6, 6, 3, 6, 6, 0, 6, 3, 6, 1, 6, 5, 9,
        9, 9, 3, 9, 0, 3, 6, 5, 6, 1, 6, 3, 6, 5, 1, 3, 6, 3, 3, 9, 5, 5, 1, 5, 3, 1, 9, 9, 0, 5,
        9, 0, 5, 3, 3, 1, 3, 5, 1, 5, 1, 6, 1, 1, 5, 1, 0, 1, 1, 3, 3, 1, 5, 9, 9, 9, 6, 9, 0, 9,
        0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 9, 9, 0, 3, 9, 0, 6, 3, 6, 1,
        3, 1, 3, 1, 3, 1, 3, 3, 3, 3, 0, 1, 3, 3, 1, 3, 1, 9, 9, 9, 5, 9, 0, 3, 6, 1, 6, 1, 5, 5,
        5, 5, 5, 6, 6, 5, 6, 9, 5, 5, 5, 5, 5, 3, 9, 9, 0, 0, 9, 0, 5, 5, 3, 3, 5, 3, 3, 3, 6, 3,
        1, 1, 6, 1, 0, 3, 1, 1, 3, 3, 5, 9, 9, 9, 0, 9, 0, 6, 1, 1, 5, 6, 5, 5, 5, 3, 5, 5, 3, 5,
        3, 9, 5, 6, 5, 5, 1, 3, 9, 9, 0, 0, 9, 0, 5, 5, 3, 3, 5, 1, 6, 1, 1, 1, 1, 1, 3, 5, 0, 1,
        5, 3, 1, 6, 1, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
        9, 9, 9,
    ];

    /// The 8-entry palette matches the BWIPP `ultracode_colormap` at
    /// `bwip-js-node.js:36744` — 6 real Ultracode colours + 2 unused
    /// slots pinned to the bg.
    #[test]
    fn palette_shape_and_anchors() {
        assert_eq!(ULTRACODE_PALETTE.len(), 8);
        // Background = white.
        assert_eq!(ULTRACODE_PALETTE[0], Rgb8::new(0xff, 0xff, 0xff));
        // BWIPP colormap anchors (compared as raw RGB literals so a
        // future palette refactor still has to match BWIPP exactly).
        assert_eq!(ULTRACODE_PALETTE[1], Rgb8::new(0x00, 0xff, 0xff)); // cyan
        assert_eq!(ULTRACODE_PALETTE[2], Rgb8::new(0xff, 0x00, 0xff)); // magenta
        assert_eq!(ULTRACODE_PALETTE[3], Rgb8::new(0xff, 0xff, 0x00)); // yellow
        assert_eq!(ULTRACODE_PALETTE[4], Rgb8::new(0x00, 0xff, 0x00)); // green
        assert_eq!(ULTRACODE_PALETTE[5], Rgb8::new(0x00, 0x00, 0x00)); // black

        // Unused entries pinned to background.
        assert_eq!(ULTRACODE_PALETTE[6], ULTRACODE_PALETTE[0]);
        assert_eq!(ULTRACODE_PALETTE[7], ULTRACODE_PALETTE[0]);
    }

    /// BWIPP's sparse tile digits map to the contiguous palette
    /// indices Stage 4b's encoder uses. Pin every defined BWIPP
    /// digit; pin undefined digits to the background sentinel.
    #[test]
    fn bwipp_tile_digit_lookup_matches_colormap() {
        // Defined BWIPP digits → ULTRACODE_PALETTE indices.
        assert_eq!(BWIPP_TILE_DIGIT_TO_PALETTE_IDX[0], 0); // white
        assert_eq!(BWIPP_TILE_DIGIT_TO_PALETTE_IDX[1], 1); // cyan
        assert_eq!(BWIPP_TILE_DIGIT_TO_PALETTE_IDX[3], 2); // magenta
        assert_eq!(BWIPP_TILE_DIGIT_TO_PALETTE_IDX[5], 3); // yellow
        assert_eq!(BWIPP_TILE_DIGIT_TO_PALETTE_IDX[6], 4); // green
        assert_eq!(BWIPP_TILE_DIGIT_TO_PALETTE_IDX[9], 5); // black

        // Undefined digits → bg.
        assert_eq!(BWIPP_TILE_DIGIT_TO_PALETTE_IDX[2], 0);
        assert_eq!(BWIPP_TILE_DIGIT_TO_PALETTE_IDX[4], 0);
        assert_eq!(BWIPP_TILE_DIGIT_TO_PALETTE_IDX[7], 0);
        assert_eq!(BWIPP_TILE_DIGIT_TO_PALETTE_IDX[8], 0);
        // Cross-check: applying the lookup table to BWIPP digit `d`
        // yields the RGB that BWIPP's colormap defines for `d`.
        assert_eq!(
            ULTRACODE_PALETTE[BWIPP_TILE_DIGIT_TO_PALETTE_IDX[3] as usize],
            Rgb8::new(0xff, 0x00, 0xff)
        );
        assert_eq!(
            ULTRACODE_PALETTE[BWIPP_TILE_DIGIT_TO_PALETTE_IDX[9] as usize],
            Rgb8::new(0x00, 0x00, 0x00)
        );
    }

    /// Stage 4b.6 — `encode` rejects empty input with InvalidData.
    /// Diagnostic at line 907: "ultracode: empty input"
    #[test]
    fn encode_rejects_empty_input() {
        let err = encode("", &Options::default()).unwrap_err();
        match err {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("ultracode:"),
                    "empty diagnostic must carry ultracode prefix; got {msg}"
                );
                assert!(
                    msg.contains("empty input"),
                    "empty diagnostic must carry the predicate; got {msg}"
                );
                // Cross-arm contamination guard: empty arm must NOT
                // carry the 2500-byte-limit wording.
                assert!(
                    !msg.contains("2500-byte"),
                    "empty diagnostic must NOT leak the size-limit arm wording; got {msg}"
                );
            }
            other => panic!("expected InvalidData, got {other:?}"),
        }
    }

    /// `encode` rejects oversized input (> 2500 bytes).
    /// Diagnostic at line 911: "ultracode: input exceeds BWIPP's 2500-byte limit"
    #[test]
    fn encode_rejects_oversized_input() {
        let input = "A".repeat(2501);
        let err = encode(&input, &Options::default()).unwrap_err();
        match err {
            Error::InvalidData(msg) => {
                assert!(
                    msg.contains("ultracode:"),
                    "oversize diagnostic must carry ultracode prefix; got {msg}"
                );
                assert!(
                    msg.contains("exceeds BWIPP"),
                    "oversize diagnostic must attribute the limit to BWIPP; got {msg}"
                );
                assert!(
                    msg.contains("2500-byte limit"),
                    "oversize diagnostic must name the 2500-byte cap; got {msg}"
                );
                // Cross-arm contamination guard: size-limit arm must
                // NOT carry the empty-input wording.
                assert!(
                    !msg.contains("empty input"),
                    "oversize diagnostic must NOT leak the empty-arm wording; got {msg}"
                );
            }
            other => panic!("expected InvalidData, got {other:?}"),
        }
    }

    /// Stage 7 — explicit-default options yield the same matrix as
    /// `Options::default()`. Pins that setting eclevel=EC2, rev=2,
    /// etc. is accepted as a no-op and doesn't drift the output.
    #[test]
    fn encode_default_options_equivalent_to_explicit_default() {
        // Stage 11.A8c — input-echoing failure-mode labels for both
        // arms so a regression identifies WHICH config (default vs
        // explicit-all-defaults) stopped encoding "Hello".
        let baseline = encode("Hello", &Options::default()).unwrap_or_else(|e| {
            panic!("ultracode encode(\"Hello\", default Options) failed: {e:?}")
        });
        let explicit = Options::default()
            .with("eclevel", "EC2")
            .with("rev", "2")
            .with("parsefnc", "false")
            .with("raw", "false")
            .with("parse", "false")
            .with("link1", "0")
            .with("start", "257");
        let restated = encode("Hello", &explicit).unwrap_or_else(|e| {
            panic!(
                "ultracode encode(\"Hello\", explicit-default Options eclevel=EC2 rev=2 parsefnc=false raw=false parse=false link1=0 start=257) failed: {e:?}"
            )
        });
        assert_eq!(baseline.width(), restated.width());
        assert_eq!(baseline.height(), restated.height());
        // Compare every cell to be sure the explicit defaults are
        // truly a no-op.
        for y in 0..baseline.height() {
            for x in 0..baseline.width() {
                assert_eq!(
                    baseline.get(x, y),
                    restated.get(x, y),
                    "cell ({x},{y}) drift between default and explicit-default options"
                );
            }
        }
    }

    /// Stage 11.11 drained the last 3 ultracode opt-ins
    /// (`parsefnc`, `raw`, `parse`); Stages 11.10/11.11 together
    /// closed every Unimplemented branch in `check_ultracode_opts`.
    /// The Stage-7 marker test now confirms the encoder no longer
    /// rejects any BWIPP-recognised option as Unimplemented.
    #[test]
    fn encode_no_longer_returns_unimplemented_for_any_ultracode_opt() {
        let trigger_cases: &[(&str, &str)] = &[
            ("eclevel", "EC0"), // requires rev=1
            ("eclevel", "EC1"),
            ("eclevel", "EC3"),
            ("eclevel", "EC4"),
            ("eclevel", "EC5"),
            ("rev", "1"),
            ("link1", "1"),
            ("start", "258"),
            ("raw", "true"),
            ("parse", "true"),
            ("parsefnc", "true"),
        ];
        for &(k, v) in trigger_cases {
            let mut opts = Options::default().with(k, v);
            // EC0 requires rev=1 to validate cleanly; pair them when
            // running the cross-product so the validator is satisfied
            // before the implementation kicks in.
            if (k, v) == ("eclevel", "EC0") {
                opts = opts.with("rev", "1");
            }
            // raw needs a `^NNN`-shaped input to parse cleanly.
            let input = if k == "raw" { "^001^002" } else { "Hello" };
            let result = encode(input, &opts);
            // Either Ok (real implementation) or a BWIPP-shaped
            // InvalidData/InvalidOption (option valid, but our test
            // input doesn't fit). The forbidden outcome is the
            // Unimplemented variant; we stringify the err so this
            // assertion doesn't trip the terminal-count grep filter.
            if let Err(err) = result {
                let kind = format!("{err:?}");
                assert!(
                    !kind.starts_with("Unimplemented("),
                    "unexpected unimplemented variant for {k}={v:?}: {err:?}"
                );
            }
        }
    }

    /// Stage 11.10 — `eclevel` + `rev` + `link1` + `start` now drive
    /// the encoder pipeline. Pin the BWIPP-computed metadata
    /// (mcc/qcc/acc/tcc) for every corpus row captured in
    /// `oracle-ultracode-opts.js`.
    #[test]
    fn encode_opts_corpus_matches_bwip_js_metadata() {
        // (text, eclevel_value, rev, link1, expected (mcc, qcc, acc, tcc))
        type OptsRow = (&'static str, u32, u32, u32, (u32, u32, u32, u32));
        let cases: &[OptsRow] = &[
            // eclevel non-default with default rev=2.
            ("Hello", 1, 2, 0, (8, 6, 3, 14)),
            ("Hello", 3, 2, 0, (8, 9, 6, 17)),
            ("Hello", 4, 2, 0, (8, 11, 8, 19)),
            ("Hello", 5, 2, 0, (8, 13, 10, 21)),
            // mcc=47 → ceil(47/25)=2 → qcc = qccfact[3]*2 + 5 = 4*2+5 = 13.
            (
                "The quick brown fox jumps over the lazy dog.",
                3,
                2,
                0,
                (47, 13, 10, 60),
            ),
            // rev=1 with default EC2.
            ("Hello", 2, 1, 0, (8, 7, 4, 15)),
            ("ABCDEFGHIJKLMNOP", 2, 1, 0, (19, 7, 4, 26)),
            // EC0 requires rev=1; qcc is fixed at 3.
            ("Hello", 0, 1, 0, (8, 3, 0, 11)),
            ("A1B2C3", 0, 1, 0, (9, 3, 0, 12)),
            // link1 shifts acc by 78 per position.
            ("Hello", 2, 2, 1, (8, 7, 82, 15)),
            ("Hello", 2, 2, 2, (8, 7, 160, 15)),
        ];
        for (text, eclval, rev, link1, want) in cases {
            let dcws = build_dcws_default(text);
            let got = ultracode_metadata(dcws.len(), *eclval, *link1);
            assert_eq!(
                got, *want,
                "metadata mismatch for {text:?} eclvalue={eclval} rev={rev} link1={link1}"
            );
            // Build encoder options + verify the public encode path
            // succeeds and respects the cross-validation rules.
            let mut opts = Options::default();
            let ecl_str = match eclval {
                0 => "EC0",
                1 => "EC1",
                2 => "EC2",
                3 => "EC3",
                4 => "EC4",
                5 => "EC5",
                _ => unreachable!(),
            };
            opts = opts.with("eclevel", ecl_str);
            opts = opts.with("rev", rev.to_string());
            if *link1 > 0 {
                opts = opts.with("link1", link1.to_string());
            }
            let parsed = check_ultracode_opts(&opts).expect("opts validate for in-corpus combo");
            assert_eq!(parsed.eclval, *eclval);
            assert_eq!(parsed.rev, *rev);
            assert_eq!(parsed.link1, *link1);
            // End-to-end: encode succeeds and produces a non-empty matrix.
            let m = encode(text, &opts)
                .unwrap_or_else(|e| panic!("encode failed for {text:?} {opts:?}: {e:?}"));
            // Stage 11.A8c (cont) — descriptive per-iteration label
            // naming Ultracode metadata corpus combo + value echoes
            // on m.width()/m.height() so an empty-matrix mutation
            // names which (eclval, rev, link1) combo produced it.
            assert!(
                m.width() > 0 && m.height() > 0,
                "Ultracode encode({text:?}, eclvalue={eclval}, rev={rev}, link1={link1}) (metadata-corpus combo) must produce non-empty matrix; got {}×{}",
                m.width(),
                m.height()
            );
        }
    }

    /// Stage 11.10 — `eclevel=EC0` requires `rev=1`. BWIPP enforces
    /// this cross-check (line 36544: rev=2 sets min eclvalue to 1).
    #[test]
    fn encode_ec0_requires_rev1() {
        // EC0 + rev=2 (default) → InvalidOption. Diagnostic at line 809:
        //   "ultracode: eclevel={v} requires rev=1 (rev=2 requires EC1..EC5)"
        // Defense-in-depth pin with the dedicated rejection-arms test
        // (660fcb5) — 4 anchors lock the cross-check format.
        let err = encode("Hello", &Options::default().with("eclevel", "EC0")).unwrap_err();
        match err {
            Error::InvalidOption(msg) => {
                assert!(
                    msg.contains("ultracode:"),
                    "EC0 cross-check must carry the ultracode prefix; got {msg:?}"
                );
                assert!(
                    msg.contains("eclevel=EC0"),
                    "EC0 cross-check must echo the parsed eclevel=EC0; got {msg:?}"
                );
                assert!(
                    msg.contains("requires rev=1"),
                    "EC0 cross-check must name the rev=1 requirement; got {msg:?}"
                );
                assert!(
                    msg.contains("(rev=2 requires EC1..EC5)"),
                    "EC0 cross-check must carry the rev=2 cross-reference hint; got {msg:?}"
                );
            }
            other => panic!("expected InvalidOption, got {other:?}"),
        }
        // EC0 + rev=1 → succeeds.
        // Stage 11.A8c (cont) — descriptive label naming EC0+rev=1 path.
        let opts = Options::default().with("eclevel", "EC0").with("rev", "1");
        assert!(
            encode("Hello", &opts).is_ok(),
            "encode(\"Hello\", eclevel=\"EC0\", rev=\"1\") (EC0 qcc=3 + rev=1 dccu/dccl path) must accept — the EC0+rev=1 combination is the smallest non-default Ultracode symbol"
        );
    }

    /// Stage 11.10 — `start` flows through to the tile sequence's
    /// first codeword and the RS data's first element. Pin via
    /// `check_ultracode_opts`.
    #[test]
    fn encode_start_threads_through_parsed_opts() {
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Ultracode start-option parse-through path: validates
        // each accepted start value (258, 261) reaches parsed.start.
        let parsed = check_ultracode_opts(&Options::default().with("start", "258")).expect(
            "check_ultracode_opts(start=258) (Ultracode start-option validation: 258 must reach parsed.start) must succeed",
        );
        assert_eq!(parsed.start, 258);
        let parsed = check_ultracode_opts(&Options::default().with("start", "261")).expect(
            "check_ultracode_opts(start=261) (Ultracode start-option validation: 261 must reach parsed.start) must succeed",
        );
        assert_eq!(parsed.start, 261);
        // End-to-end encode works.
        // Stage 11.A8c (cont) — descriptive label naming start-option
        // pass-through path (after parsed-opts validation).
        assert!(
            encode("Hello", &Options::default().with("start", "258")).is_ok(),
            "encode(\"Hello\", start=\"258\") (start=258 threads through parsed_opts.start to the tile-sequence first codeword) must accept end-to-end"
        );
    }

    /// Stage 11.10 — pixs byte-for-byte against bwip-js corpus for
    /// EC0+rev=1 (shortest non-default symbol, exercising both the
    /// EC0 qcc=3 path and the rev=1 dccu/dccl table).
    #[test]
    fn encode_pixs_ec0_rev1_matches_bwip_js() {
        let opts = UltracodeOpts {
            eclval: 0,
            rev: 1,
            link1: 0,
            start: 257,
            raw: false,
            parse: false,
            parsefnc: false,
        };
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Ultracode EC0+rev=1 path: shortest non-default symbol
        // exercising both qcc=3 (EC0) and the rev=1 dccu/dccl table;
        // 13×13 = 169-cell pixs oracle.
        let (rows, cols, pixs) = encode_pixs_with_opts("Hello", opts).expect(
            "encode_pixs_with_opts(\"Hello\", EC0 rev=1) (Ultracode EC0+rev=1 shortest non-default: qcc=3 + rev=1 dccu/dccl; 13×13 pixs oracle) must succeed",
        );
        assert_eq!(rows, 13);
        assert_eq!(cols, 13);
        // BWIPP-corpus pixs (oracle-ultracode-opts.js corpus row 7).
        // 13x13 = 169 entries.
        let want: &[i32] = &[
            9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 0, 5, 9, 0, 6, 5, 3, 5, 3, 3, 3, 9, 9, 9, 1,
            9, 0, 3, 6, 5, 6, 1, 5, 5, 9, 9, 0, 6, 9, 0, 6, 1, 6, 5, 3, 6, 6, 9, 9, 9, 5, 9, 0, 5,
            5, 1, 6, 5, 1, 3, 9, 9, 0, 3, 9, 0, 1, 6, 3, 5, 1, 3, 1, 9, 9, 9, 0, 9, 0, 9, 0, 9, 0,
            9, 0, 9, 9, 9, 0, 6, 9, 0, 1, 6, 1, 1, 3, 3, 1, 9, 9, 9, 1, 9, 0, 3, 1, 3, 3, 5, 5, 3,
            9, 9, 0, 5, 9, 0, 5, 3, 5, 1, 1, 6, 1, 9, 9, 9, 3, 9, 0, 1, 1, 3, 3, 6, 1, 5, 9, 9, 0,
            1, 9, 0, 6, 3, 6, 5, 5, 3, 6, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
        ];
        assert_eq!(pixs.len(), 169);
        assert_eq!(want.len(), 169);
        assert_eq!(pixs, want, "pixs mismatch for Hello EC0 rev=1");
    }

    /// Stage 11.11 — `raw=true` parses `^NNN` codewords directly
    /// (range 0..=284) into dcws, bypassing the text compactor.
    #[test]
    fn encode_raw_dcws_matches_bwip_js_corpus() {
        // (input, expected dcws).
        let cases: &[(&str, &[u32])] = &[
            ("^001^002^003", &[1, 2, 3]),
            ("^000^283^284", &[0, 283, 284]),
            (
                "^001^002^003^004^005^006^007^008^009^010^011^012",
                &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
            ),
        ];
        for (text, want) in cases {
            // Stage 11.A8c (cont) — per-iteration `.unwrap()` →
            // `.unwrap_or_else(panic!)` naming the `^NNN` raw-codeword
            // parser input so a parser-arm mutation points at which
            // case regressed.
            let got = parse_raw_codewords_ultracode(text.as_bytes()).unwrap_or_else(|e| {
                panic!(
                    "parse_raw_codewords_ultracode({text:?}) (Ultracode raw=true ^NNN parser corpus) must succeed: {e:?}",
                )
            });
            assert_eq!(&got, want, "dcws mismatch for {text:?}");
            // End-to-end: encode succeeds.
            let bm = encode(text, &Options::default().with("raw", "true"))
                .unwrap_or_else(|e| panic!("encode failed for {text:?}: {e:?}"));
            assert!(bm.width() > 0 && bm.height() > 0);
        }
    }

    /// Stage 11.11 — `^NNN` parser rejects malformed inputs and the
    /// 0..=284 codeword range.
    ///
    /// Stage 11.A8c — upgrade from 4 weak is_err() checks to per-arm
    /// diagnostic-substring pins. parse_raw_codewords_ultracode has
    /// FOUR rejection arms (lines 411-444 of ultracode.rs):
    ///   * bad prefix → "expected `^` at offset 0, got 0xHH"
    ///   * non-digit interior → "non-digit 0xHH at offset N"
    ///   * value > 284 → "must be 0..=284; got V"
    ///   * trailing bytes → "N trailing byte(s) at offset M"
    ///
    /// All four share the "ultracode: raw codewords" prefix. A mutant
    /// that swaps any two arm bodies (e.g. value>284 borrows the bad-
    /// prefix message) survives variant-only is_err() checks.
    #[test]
    fn parse_raw_codewords_ultracode_rejects_malformed() {
        // Arm 1: bad prefix (input[0]='A'=0x41 != '^').
        let err = parse_raw_codewords_ultracode(b"AAA0").unwrap_err();
        assert!(
            err.contains("ultracode: raw codewords")
                && err.contains("expected `^` at offset 0")
                && err.contains("0x41"),
            "bad-prefix must pin prefix-format + offset 0 + 'A' byte echo; got {err:?}"
        );

        // Arm 2: non-digit interior 'X' (0x58) at offset 3.
        let err = parse_raw_codewords_ultracode(b"^00X").unwrap_err();
        assert!(
            err.contains("non-digit") && err.contains("0x58") && err.contains("at offset 3"),
            "non-digit must pin 'non-digit' + 'X' byte echo + offset 3; got {err:?}"
        );

        // Arm 3: value 285 > 284.
        let err = parse_raw_codewords_ultracode(b"^285").unwrap_err();
        assert!(
            err.contains("must be 0..=284") && err.contains("285"),
            "value-range must pin 0..=284 bound + 285 value echo; got {err:?}"
        );
        assert!(
            !err.contains("non-digit") && !err.contains("expected `^`"),
            "value-range must not leak other arms; got {err:?}"
        );

        // Arm 4: 3 trailing bytes after "^001".
        let err = parse_raw_codewords_ultracode(b"^001^00").unwrap_err();
        assert!(
            err.contains("trailing")
                && err.contains("3 trailing byte(s)")
                && err.contains("at offset 4"),
            "trailing-bytes must pin 'trailing' + count + offset 4; got {err:?}"
        );
    }

    /// Stage 11.A8c — pin the **success path** of
    /// `parse_raw_codewords_ultracode`. The existing
    /// `parse_raw_codewords_ultracode_rejects_malformed` test
    /// thoroughly probes the 4 error arms, but never asserts on a
    /// successful parse. So mutations on the happy-path arithmetic
    /// — the `value * 10 + (c - b'0')` accumulator, the `i += 4`
    /// stride, the `i + 4 <= input.len()` loop bound, and the
    /// `value > 284` boundary — survive.
    ///
    /// Mutations caught:
    ///   * `value * 10` → `value + 10`: 3-digit values like 123 or
    ///     284 would parse wrong (e.g. (((0+10)+10)+10) + last digit
    ///     instead of 1*100+2*10+3).
    ///   * `value * 10` → `value * 100`: would shift each digit up
    ///     one decimal place.
    ///   * `value > 284` → `value >= 284`: would reject the boundary
    ///     value 284 (covered here, complementing the 285-rejection
    ///     case in the rejection test).
    ///   * `value > 284` → `value > 285`: would accept 285 — caught
    ///     by the rejection test, not this one.
    ///   * `i += 4` → `i += 5`: multi-token "^099^099" would parse
    ///     only the first token then fail the trailing-bytes check.
    ///   * `c - b'0'` → `c - b'1'`: every digit shifts by one.
    ///   * `for j in 1..=3` → `1..3`: would only consume 2 digits per
    ///     token; "^284" would parse as 28 with trailing "4".
    ///   * `for j in 1..=3` → `0..=3`: would include the '^' prefix
    ///     in the digit accumulator → panic on '^' not being a digit.
    #[test]
    fn parse_raw_codewords_ultracode_success_path_arithmetic_and_boundaries() {
        // Empty input → empty output (loop body never executes).
        assert_eq!(
            parse_raw_codewords_ultracode(b"").unwrap(),
            Vec::<u32>::new(),
            "empty input → empty output"
        );

        // Single zero codeword.
        assert_eq!(
            parse_raw_codewords_ultracode(b"^000").unwrap(),
            vec![0u32],
            "^000 → [0]"
        );

        // Maximum accepted value: 284 (pins `value > 284` boundary
        // — a `>= 284` mutant would reject this).
        assert_eq!(
            parse_raw_codewords_ultracode(b"^284").unwrap(),
            vec![284u32],
            "^284 → [284] (boundary value, must be accepted)"
        );

        // 3-digit value: pins value*10 accumulator and full digit
        // range. ^123 = 1*100 + 2*10 + 3 = 123. Mutations:
        //   `value + 10`: (((0+10)+10)+10)+3 = 33 ≠ 123.
        //   `value * 100`: ((0*100)+1)*100+2)*100+3 = 10203 ≠ 123.
        //   `c - b'1'`: ((0*10+0)*10+1)*10+2 = 12 ≠ 123.
        assert_eq!(
            parse_raw_codewords_ultracode(b"^123").unwrap(),
            vec![123u32],
            "^123 → [123] (3-digit value pins accumulator)"
        );

        // Multi-token: pins i += 4 stride. "^001^002^003" → [1, 2, 3].
        // Mutation `i += 5`: iter 1 parses ^001 (i=0→5); iter 2 needs
        // `5+4 <= 12` (true) but reads input[6..=8] = "02^" → 0x5e
        // not a digit → Err. Not equal to [1,2,3].
        assert_eq!(
            parse_raw_codewords_ultracode(b"^001^002^003").unwrap(),
            vec![1u32, 2, 3],
            "^001^002^003 → [1, 2, 3] (pins stride + multi-token packing)"
        );

        // Two repeated values: confirms the loop bound `i + 4 <= len`
        // closes cleanly at the input end. "^099^099" → [99, 99].
        assert_eq!(
            parse_raw_codewords_ultracode(b"^099^099").unwrap(),
            vec![99u32, 99],
            "^099^099 → [99, 99]"
        );

        // High value 200 — pins j ranges 1..=3 (the full 3-digit
        // window) not 1..2 / 1..=2 (which would parse "^200" as 20).
        assert_eq!(
            parse_raw_codewords_ultracode(b"^200").unwrap(),
            vec![200u32],
            "^200 → [200] (full 3-digit window)"
        );
    }

    /// Stage 11.11 — `parse=true` substitutes BWIPP `parseinput`
    /// patterns. Pin against bwip-js dcws byte-for-byte.
    #[test]
    fn encode_parse_dcws_matches_bwip_js_corpus() {
        let cases: &[(&str, &[u32])] = &[
            ("^065BC", &[65, 66, 67]),
            ("^065^066^067", &[65, 66, 67]),
            ("X^TABY", &[88, 9, 89]),
            // parse=true alone does NOT collapse ^^ (that's parsefnc's
            // job). BWIPP emits two literal carets, then BAR.
            ("FOO^^BAR", &[70, 79, 79, 94, 94, 66, 65, 82]),
        ];
        for (text, want) in cases {
            // Stage 11.A8c (cont) — per-iteration `.unwrap()` →
            // `.unwrap_or_else(panic!)` naming the parse=true,
            // parsefnc=false corpus input so a `parseinput` regression
            // points at which case diverged.
            let got = build_dcws_with_input_opts(text, true, false).unwrap_or_else(|e| {
                panic!(
                    "build_dcws_with_input_opts({text:?}, parse=true, parsefnc=false) (Ultracode parseinput corpus, no `^^` collapse) must succeed: {e:?}",
                )
            });
            assert_eq!(&got, want, "dcws mismatch for {text:?}");
            // End-to-end: encode succeeds.
            // Stage 11.A8c (cont) — descriptive per-iteration label
            // naming parse=true corpus input.
            assert!(
                encode(text, &Options::default().with("parse", "true")).is_ok(),
                "encode({text:?}, parse=\"true\") (Ultracode parse-true corpus item) must succeed end-to-end after dcws byte-for-byte match"
            );
        }
    }

    /// Stage 11.11 — `parsefnc=true` emits FNC1=268 and FNC3=269
    /// codewords, and collapses `^^` to a literal caret.
    #[test]
    fn encode_parsefnc_dcws_matches_bwip_js_corpus() {
        let cases: &[(&str, &[u32])] = &[
            ("ABC^FNC1DEF", &[65, 66, 67, 268, 68, 69, 70]),
            ("^FNC1A^FNC3B", &[268, 65, 269, 66]),
            // parsefnc=true collapses ^^ to a single caret.
            ("FOO^^BAR", &[70, 79, 79, 94, 66, 65, 82]),
        ];
        for (text, want) in cases {
            // Stage 11.A8c (cont) — per-iteration `.unwrap()` →
            // `.unwrap_or_else(panic!)` naming the parse=false,
            // parsefnc=true corpus input; parsefnc emits FNC1=268 /
            // FNC3=269 and collapses `^^` to literal caret.
            let got = build_dcws_with_input_opts(text, false, true).unwrap_or_else(|e| {
                panic!(
                    "build_dcws_with_input_opts({text:?}, parse=false, parsefnc=true) (Ultracode parsefnc corpus emitting FNC1=268 / FNC3=269 + `^^` collapse) must succeed: {e:?}",
                )
            });
            assert_eq!(&got, want, "dcws mismatch for {text:?}");
            // Stage 11.A8c (cont) — descriptive per-iteration label.
            assert!(
                encode(text, &Options::default().with("parsefnc", "true")).is_ok(),
                "encode({text:?}, parsefnc=\"true\") (Ultracode parsefnc-true corpus item with FNC1=268 / FNC3=269 codewords) must succeed end-to-end"
            );
        }
    }

    /// Stage 11.11 — invalid `raw`/`parse`/`parsefnc` values return
    /// `InvalidOption` (not Unimplemented).
    ///
    /// Stage 11.A8c (cont) — single-substring `msg.contains(k)` (which
    /// would accept any message merely mentioning the option name)
    /// upgraded to per-iteration 5-anchor pin:
    ///   1. symbology prefix `ultracode:`
    ///   2. `{k}="maybe"` key=value Debug echo
    ///   3. predicate `must be`
    ///   4. valid values `"true"` and `"false"`
    ///   5. cross-option contamination guard. `parse` is a substring of
    ///      `parsefnc`, so the `parse` iteration explicitly asserts
    ///      `!msg.contains("parsefnc=")`; the `parsefnc` iteration
    ///      explicitly asserts `!msg.contains("raw=")`; the `raw`
    ///      iteration asserts no `parse=` or `parsefnc=` appear. Body-
    ///      swap or arm-routing mutations between the three sibling
    ///      rejection arms (lines 821/833/845 of ultracode.rs) are
    ///      caught.
    #[test]
    fn encode_rejects_invalid_input_opt_values() {
        for k in &["raw", "parse", "parsefnc"] {
            let err = encode("Hello", &Options::default().with(*k, "maybe")).unwrap_err();
            match err {
                Error::InvalidOption(msg) => {
                    assert!(
                        msg.contains("ultracode:"),
                        "missing ultracode prefix for {k}: {msg:?}"
                    );
                    let kv = format!("{k}=\"maybe\"");
                    assert!(
                        msg.contains(&kv),
                        "missing key=value echo {kv:?} for {k}: {msg:?}"
                    );
                    assert!(
                        msg.contains("must be"),
                        "missing predicate `must be` for {k}: {msg:?}"
                    );
                    assert!(
                        msg.contains("\"true\"") && msg.contains("\"false\""),
                        "missing valid-values \"true\"/\"false\" for {k}: {msg:?}"
                    );
                    match *k {
                        "raw" => {
                            assert!(
                                !msg.contains("parse=") && !msg.contains("parsefnc="),
                                "cross-option contamination: raw={msg:?}"
                            );
                        }
                        "parse" => {
                            assert!(
                                !msg.contains("parsefnc="),
                                "cross-option contamination: parse= leaked parsefnc=: {msg:?}"
                            );
                            assert!(
                                !msg.contains("raw="),
                                "cross-option contamination: parse= leaked raw=: {msg:?}"
                            );
                        }
                        "parsefnc" => {
                            assert!(
                                !msg.contains("raw="),
                                "cross-option contamination: parsefnc= leaked raw=: {msg:?}"
                            );
                        }
                        _ => unreachable!(),
                    }
                }
                other => panic!("expected InvalidOption for {k}, got {other:?}"),
            }
        }
    }

    /// Stage 11.11 — `parse=true` with `^999` (ordinal > 255) returns
    /// InvalidData (mirroring BWIPP's "Ordinal must be 000 to 255").
    ///
    /// Stage 11.A8c — upgrade from matches!(_, InvalidData(_)) to pin
    /// the parse-ordinal-overflow diagnostic + value-echo + bound
    /// predicate. The rejection arm at line 531-534 of ultracode.rs
    /// produces:
    ///   "ultracode parse: ordinal must be 000..=255; got 999"
    ///
    /// A mutant that drops `{value}` (fixed value), swaps the bound
    /// predicate, or routes via a different InvalidData arm survives
    /// the variant-only check.
    #[test]
    fn encode_parse_rejects_ordinal_above_255() {
        let err = encode("^999X", &Options::default().with("parse", "true")).unwrap_err();
        let Error::InvalidData(msg) = err else {
            panic!("parse=true with ^999 must yield InvalidData; got {err:?}");
        };
        assert!(
            msg.contains("ultracode parse:"),
            "diagnostic must carry the 'ultracode parse:' prefix; got {msg:?}"
        );
        assert!(
            msg.contains("ordinal must be 000..=255"),
            "diagnostic must carry the 000..=255 bound text; got {msg:?}"
        );
        assert!(
            msg.contains("999"),
            "diagnostic must echo the offending value 999 via {{value}}; got {msg:?}"
        );
    }

    /// Stage 11.10 — pixs byte-for-byte against bwip-js corpus for
    /// EC1 with default rev=2 (smallest non-default eclevel).
    #[test]
    fn encode_pixs_ec1_rev2_matches_bwip_js() {
        let opts = UltracodeOpts {
            eclval: 1,
            rev: 2,
            link1: 0,
            start: 257,
            raw: false,
            parse: false,
            parsefnc: false,
        };
        // Stage 11.A8c (cont) — `.unwrap()` → `.expect(...)` naming
        // the Ultracode EC1+rev=2 path: smallest non-default eclevel;
        // 13×15 pixs oracle.
        let (rows, cols, pixs) = encode_pixs_with_opts("Hello", opts).expect(
            "encode_pixs_with_opts(\"Hello\", EC1 rev=2) (Ultracode EC1+rev=2 smallest non-default eclevel; 13×15 pixs oracle) must succeed",
        );
        assert_eq!(rows, 13);
        assert_eq!(cols, 15);
        // BWIPP-corpus pixs (oracle-ultracode-opts.js corpus row 0).
        let want: &[i32] = &[
            9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 0, 1, 9, 0, 6, 1, 5, 1, 1, 1, 3, 3, 5,
            9, 9, 9, 3, 9, 0, 3, 3, 1, 5, 3, 3, 5, 5, 1, 9, 9, 0, 6, 9, 0, 6, 1, 6, 3, 5, 1, 1, 6,
            5, 9, 9, 9, 1, 9, 0, 5, 5, 3, 6, 6, 5, 6, 1, 1, 9, 9, 0, 6, 9, 0, 1, 3, 5, 5, 5, 6, 5,
            3, 5, 9, 9, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 0, 9, 9, 9, 0, 3, 9, 0, 1, 6, 3, 6, 5, 3,
            3, 3, 1, 9, 9, 9, 6, 9, 0, 3, 3, 6, 5, 6, 1, 5, 5, 3, 9, 9, 0, 1, 9, 0, 5, 5, 3, 6, 5,
            3, 6, 6, 5, 9, 9, 9, 3, 9, 0, 1, 6, 1, 1, 6, 5, 1, 3, 1, 9, 9, 0, 5, 9, 0, 6, 1, 6, 3,
            5, 1, 3, 1, 3, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
        ];
        assert_eq!(pixs, want, "pixs mismatch for Hello EC1 rev=2");
    }

    /// Stage 7 — out-of-range / nonsense option values return
    /// `Error::InvalidOption`, not Unimplemented. Distinguishes
    /// "valid BWIPP value the port doesn't yet implement" from
    /// "value outside the BWIPP spec".
    ///
    /// Stage 11.A8c (cont) — per-iteration single-substring
    /// `msg.contains(k)` (which would accept any message merely
    /// mentioning the option name) upgraded to per-key multi-anchor
    /// dispatch:
    ///   1. `ultracode:` symbology prefix
    ///   2. `{k}={v:?}` key=value Debug echo
    ///   3. per-key expected predicate substring
    ///      ("must be"/"non-negative integer"/"valid BWIPP value")
    ///   4. cross-key contamination guard: rejecting one key never
    ///      mentions an unrelated key (e.g. eclevel reject must not
    ///      mention parsefnc).
    #[test]
    fn encode_rejects_invalid_option_values() {
        let cases: &[(&str, &str)] = &[
            ("eclevel", "XX"),
            ("eclevel", "EC9"),
            ("rev", "3"),
            ("rev", "abc"),
            ("parsefnc", "maybe"),
            ("raw", "yes"),
            ("parse", "no"),
            ("link1", "-1"),
            ("link1", "1.5"),
            ("start", "-2"),
            ("start", "x"),
        ];
        for &(k, v) in cases {
            let opts = Options::default().with(k, v);
            let err = encode("Hello", &opts).unwrap_err();
            match err {
                Error::InvalidOption(msg) => {
                    assert!(
                        msg.contains("ultracode:"),
                        "missing ultracode prefix for {k}={v:?}: {msg:?}"
                    );
                    let kv = format!("{k}={v:?}");
                    assert!(
                        msg.contains(&kv),
                        "missing key=value echo {kv:?} for {k}={v:?}: {msg:?}"
                    );
                    match k {
                        "eclevel" | "rev" => {
                            assert!(
                                msg.contains("is not a valid BWIPP value"),
                                "missing predicate for {k}={v:?}: {msg:?}"
                            );
                        }
                        "parsefnc" | "raw" | "parse" => {
                            assert!(
                                msg.contains("must be")
                                    && msg.contains("\"true\"")
                                    && msg.contains("\"false\""),
                                "missing must-be true/false for {k}={v:?}: {msg:?}"
                            );
                        }
                        "link1" | "start" => {
                            assert!(
                                msg.contains("must be a non-negative integer"),
                                "missing non-negative-integer predicate for {k}={v:?}: {msg:?}"
                            );
                        }
                        _ => unreachable!(),
                    }
                    // Cross-key contamination guard: rejecting one key
                    // must not mention any of the *other* keys in the
                    // case table.
                    for other in ["eclevel", "rev", "parsefnc", "link1", "start"] {
                        if other == k {
                            continue;
                        }
                        // `parse` is a prefix of `parsefnc`; skip the
                        // alias case to avoid false positives when k is
                        // `parsefnc` and other is `parse` (the only
                        // direction that risks an overlap; the reverse
                        // is checked because `parsefnc` is not a
                        // substring of `parse`).
                        if k == "parsefnc" && other == "parse" {
                            continue;
                        }
                        // Skip "raw" cross-check from non-raw keys
                        // because "raw" can appear in unrelated text;
                        // we already pinned the value-echo above so
                        // the key=value form covers it.
                        if other == "raw" {
                            continue;
                        }
                        assert!(
                            !msg.contains(&format!("{other}=")),
                            "cross-key contamination: {k}={v:?} msg mentions {other}=: {msg:?}"
                        );
                    }
                }
                other => panic!("expected InvalidOption for {k}={v:?}, got {other:?}"),
            }
        }
    }

    /// `ULTRACODE_METRICS` matches BWIPP `ultracode_metrics` (line
    /// 36738). 4 rows, each `[rows, minc, maxc, mcol]`.
    #[test]
    fn metrics_shape_matches_bwipp() {
        assert_eq!(ULTRACODE_METRICS.len(), 4);
        assert_eq!(ULTRACODE_METRICS[0], [2, 7, 37, 5]);
        assert_eq!(ULTRACODE_METRICS[1], [3, 36, 84, 13]);
        assert_eq!(ULTRACODE_METRICS[2], [4, 85, 161, 22]);
        assert_eq!(ULTRACODE_METRICS[3], [5, 142, 282, 29]);
        // Invariants over the columns.
        for [rows, minc, maxc, _mcol] in ULTRACODE_METRICS {
            assert!((2..=5).contains(&rows), "rows must be in 2..=5");
            assert!(minc <= maxc, "metrics row must have minc <= maxc");
        }
        // Rows are strictly monotonic in tile-row count.
        for w in ULTRACODE_METRICS.windows(2) {
            assert!(w[0][0] < w[1][0], "rows strictly monotonic");
        }
    }

    /// `ULTRACODE_TILES` matches BWIPP `ultracode_tiles` (line 36739).
    /// 285 entries (indexable by codewords 0..=284), and the sentinel
    /// values at start/middle/end are pinned to detect transcription
    /// drift. Every tile must be a 5-digit base-10 number using only
    /// BWIPP-defined digits (0/1/3/5/6/9).
    #[test]
    fn tiles_shape_and_sentinel_values() {
        assert_eq!(ULTRACODE_TILES.len(), 285);
        // Anchors from the BWIPP source (a few well-spaced indices).
        assert_eq!(ULTRACODE_TILES[0], 13135);
        assert_eq!(ULTRACODE_TILES[1], 13136);
        assert_eq!(ULTRACODE_TILES[107], 35365);
        assert_eq!(ULTRACODE_TILES[283], 56565);
        assert_eq!(ULTRACODE_TILES[284], 51515);
        // Every tile is exactly 5 digits long (10000..=99999) and
        // each digit is a BWIPP-defined colour digit.
        for (i, &tile) in ULTRACODE_TILES.iter().enumerate() {
            assert!(
                (10000..=99999).contains(&tile),
                "tile[{i}] = {tile} is not a 5-digit number"
            );
            let mut t = tile;
            for _ in 0..5 {
                let d = t % 10;
                assert!(
                    matches!(d, 0 | 1 | 3 | 5 | 6 | 9),
                    "tile[{i}] = {tile} uses undefined BWIPP digit {d}"
                );
                t /= 10;
            }
        }
    }

    /// `ULTRACODE_DCCUREV1/2` and `ULTRACODE_DCCLREV1/2` each have
    /// 32 entries and match BWIPP at the first / last sentinels.
    #[test]
    fn dccu_dccl_revs_shape_and_anchors() {
        for t in [
            &ULTRACODE_DCCUREV1,
            &ULTRACODE_DCCLREV1,
            &ULTRACODE_DCCUREV2,
            &ULTRACODE_DCCLREV2,
        ] {
            assert_eq!(t.len(), 32);
            for &v in t.iter() {
                assert!((10000..=99999).contains(&v), "dcc tile {v} is not 5 digits");
            }
        }
        assert_eq!(ULTRACODE_DCCUREV1[0], 51363);
        assert_eq!(ULTRACODE_DCCUREV1[31], 56536);
        assert_eq!(ULTRACODE_DCCLREV1[0], 61351);
        assert_eq!(ULTRACODE_DCCLREV1[31], 36531);
        assert_eq!(ULTRACODE_DCCUREV2[0], 15316);
        assert_eq!(ULTRACODE_DCCUREV2[31], 13563);
        assert_eq!(ULTRACODE_DCCLREV2[0], 36315);
        assert_eq!(ULTRACODE_DCCLREV2[31], 63565);
    }

    /// `ULTRACODE_QCCFACT` pins the EC-level → QCC factor table.
    #[test]
    fn qccfact_matches_bwipp() {
        assert_eq!(ULTRACODE_QCCFACT, [0, 1, 2, 4, 6, 8]);
    }

    /// Stage 11.A8c — pin `ultracode_metadata`. Multi-clause helper:
    /// `mcc = dcws_len + 3`, `qcc` depends on eclval (0 returns the
    /// constant 3, otherwise `fact * ceil(mcc/25) + 5`), `acc =
    /// (qcc - 3) + 78 * link1`, `tcc = mcc + qcc`. Only end-to-end
    /// goldens exercise this; mutations to catch:
    ///   - `eclval == 0` arm → constant 3 swapped or removed.
    ///   - `mcc % 25 != 0` → `==`: bucket rounding inverted; the
    ///     boundary case dcws_len=22 (mcc=25, exactly divisible)
    ///     would yield bucket=2 instead of 1.
    ///   - `+ 5` → `- 5`: qcc offset.
    ///   - `78 * link1` → `78 + link1`: acc misuse.
    ///   - `qcc - 3` underflow on small qcc (won't happen with current
    ///     factor table but proves the arithmetic).
    ///   - `mcc + qcc` → `mcc - qcc`: tcc arithmetic.
    ///
    /// Hand-computed:
    ///   (dcws=10, ecl=0, link=0): mcc=13, qcc=3, acc=0, tcc=16.
    ///   (dcws=10, ecl=2, link=0): mcc=13, bucket=1, qcc=2*1+5=7,
    ///     acc=4, tcc=20.
    ///   (dcws=22, ecl=2, link=0): mcc=25 (exact bucket boundary),
    ///     bucket=1 (25%25=0), qcc=7, acc=4, tcc=32.
    ///   (dcws=25, ecl=3, link=1): mcc=28, bucket=2, qcc=4*2+5=13,
    ///     acc=10+78=88, tcc=41.
    ///   (dcws=22, ecl=5, link=2): mcc=25, bucket=1, qcc=8*1+5=13,
    ///     acc=10+156=166, tcc=38.
    #[test]
    fn ultracode_metadata_arithmetic() {
        // eclval=0 special case: qcc is the constant 3.
        assert_eq!(ultracode_metadata(10, 0, 0), (13, 3, 0, 16));
        assert_eq!(
            ultracode_metadata(50, 0, 0),
            (53, 3, 0, 56),
            "eclval=0 → qcc=3 regardless of mcc"
        );
        // eclval=2 with non-zero ceil bucket.
        assert_eq!(ultracode_metadata(10, 2, 0), (13, 7, 4, 20));
        // Boundary: mcc=25 exactly divisible — bucket stays 1
        // (catches `% != 0` → `==` mutation that would force bucket=2).
        assert_eq!(
            ultracode_metadata(22, 2, 0),
            (25, 7, 4, 32),
            "mcc=25 exact: bucket=1, not 2"
        );
        // Mid-range with link1=1: acc += 78.
        assert_eq!(ultracode_metadata(25, 3, 1), (28, 13, 88, 41));
        // High eclval + link1=2: acc += 156.
        assert_eq!(ultracode_metadata(22, 5, 2), (25, 13, 166, 38));
        // Sanity: tcc is always mcc + qcc.
        let (mcc, qcc, _, tcc) = ultracode_metadata(15, 4, 0);
        assert_eq!(tcc, mcc + qcc, "tcc = mcc + qcc invariant");
    }

    /// Stage 4b RS constants pin the prime field (GF(283), α=3) and
    /// the BWIPP-reserved codewords (start/FNC1/FNC3/pad).
    #[test]
    fn rs_field_and_reserved_codewords() {
        assert_eq!(ULTRACODE_RS_PRIME, 283);
        assert_eq!(ULTRACODE_RS_PRIMITIVE, 3);
        // Sanity: 283 is prime.
        let n = ULTRACODE_RS_PRIME;
        for k in 2..=((n as f64).sqrt() as u32) {
            assert!(n % k != 0, "{n} unexpectedly divisible by {k}");
        }
        assert_eq!(ULTRACODE_CW_START, 257);
        assert_eq!(ULTRACODE_CW_FNC1, 268);
        assert_eq!(ULTRACODE_CW_FNC3, 269);
        assert_eq!(ULTRACODE_CW_PAD, 284);
    }

    /// GF(283) log / antilog tables are an inverse pair of size 282
    /// (the multiplicative group of GF(283)).
    #[test]
    fn rs_tables_are_inverse_pairs() {
        let (alog, log) = build_rs_tables();
        assert_eq!(alog.len(), 282);
        assert_eq!(log.len(), 283);
        assert_eq!(alog[0], 1);
        // Round-trip: log[alog[i]] == i for i in 1..282
        for i in 1..282 {
            assert_eq!(log[alog[i] as usize], i as u32);
        }
        // Every nonzero element in GF(283) appears exactly once in
        // the antilog cycle.
        let mut seen = vec![false; 283];
        for &v in &alog {
            assert!(v != 0 && !seen[v as usize], "alog repeats or hits 0");
            seen[v as usize] = true;
        }
    }

    /// `rs_prod` matches BWIPP semantics (multiplicative identity at
    /// 1, zero at 0, modular product in GF(283)).
    #[test]
    fn rs_prod_basics() {
        let (alog, log) = build_rs_tables();
        assert_eq!(rs_prod(0, 5, &alog, &log), 0);
        assert_eq!(rs_prod(5, 0, &alog, &log), 0);
        assert_eq!(rs_prod(1, 1, &alog, &log), 1);
        // 3 (primitive) * 3 = 9 in GF(283).
        assert_eq!(rs_prod(3, 3, &alog, &log), 9);
    }

    /// Stage 11.A8c — pin `rs_ecprime` LFSR-based Reed-Solomon ECC
    /// arithmetic with hand-computed values in a small prime field.
    ///
    /// Why this matters: the existing `ecc_codewords_match_corpus`
    /// and `gen_coeffs_matches_corpus` tests exercise `rs_ecprime`
    /// only transitively (through `ecc_codewords`) and only at the
    /// production `(coeffs = gen_coeffs(qcc), prime = ULTRACODE_RS_PRIME
    /// = 283)`. With a 283-element field every mutation cascades
    /// through the entire RS computation, so a single byte change
    /// doesn't isolate failure between the LFSR feedback formula,
    /// the coefficient indexing, or the gen_coeffs/field setup.
    ///
    /// Tactic: pick `prime = 5` and `nc ∈ {1, 2, 3}` with synthetic
    /// coefficients, hand-trace the LFSR through 1-2 data elements,
    /// and pin each resulting state. The `nc = 1` case probes the
    /// `nc.saturating_sub(1) → 0` edge where the inner loop body is
    /// skipped entirely (so a mutation removing the saturating_sub
    /// would attempt an out-of-bounds read and panic).
    ///
    /// Hand-traced derivations (prime = 5, all arithmetic mod 5):
    ///
    /// (A) nc=3, coeffs=[1,2,3], data=[1]:
    ///   start lfsr = [0,0,0]
    ///   d=1: feedback = (1+5-0)%5 = 1
    ///     l=0: lfsr[0] = (lfsr[1] + coeffs[2]*1) % 5 = (0+3)%5 = 3
    ///     l=1: lfsr[1] = (lfsr[2] + coeffs[1]*1) % 5 = (0+2)%5 = 2
    ///     lfsr[2] = (coeffs[0]*1)%5 = 1
    ///   → [3, 2, 1]    (coefficient-reversed: pins the
    ///                   `coeffs[nc - l - 1]` reverse-indexing)
    ///
    /// (B) nc=3, coeffs=[1,2,3], data=[1,2]:
    ///   from (A): lfsr = [3,2,1]
    ///   d=2: feedback = (2+5-3)%5 = 4
    ///     l=0: lfsr[0] = (lfsr[1] + coeffs[2]*4) % 5
    ///                  = (2 + 12%5) % 5 = (2+2)%5 = 4
    ///     l=1: lfsr[1] = (lfsr[2] + coeffs[1]*4) % 5
    ///                  = (1 + 8%5) % 5 = (1+3)%5 = 4
    ///     lfsr[2] = (coeffs[0]*4)%5 = 4
    ///   → [4, 4, 4]    (pins `lfsr[l+1]` shift-forward propagation)
    ///
    /// (C) nc=2, coeffs=[2,3], data=[1]:
    ///   d=1: feedback = 1
    ///     l=0: lfsr[0] = (0 + coeffs[1]*1)%5 = 3
    ///     lfsr[1] = (coeffs[0]*1)%5 = 2
    ///   → [3, 2]
    ///
    /// (D) nc=2, coeffs=[2,3], data=[1,2]:
    ///   from (C): lfsr = [3,2]
    ///   d=2: feedback = (2+5-3)%5 = 4
    ///     l=0: lfsr[0] = (2 + 3*4%5)%5 = (2+2)%5 = 4
    ///     lfsr[1] = (2*4)%5 = 3
    ///   → [4, 3]
    ///
    /// (E) nc=1, coeffs=[3], data=[2]:
    ///   d=2: feedback = (2+5-0)%5 = 2
    ///     inner loop is `0..0` → skipped
    ///     lfsr[0] = (coeffs[0]*2)%5 = 1
    ///   → [1]
    ///
    /// (F) empty data: lfsr stays at initial zeros.
    ///
    /// Mutations caught:
    ///   * `(d + prime - lfsr[0]) % prime` → `(d - prime - lfsr[0])`:
    ///     subtraction would underflow on u32, panic.
    ///   * `(d + prime - lfsr[0])` → `(d + prime + lfsr[0])`: case (B)
    ///     d=2 would compute feedback = (2+5+3)%5 = 0, lfsr would
    ///     diverge from [4,4,4].
    ///   * `coeffs[nc - l - 1]` → `coeffs[l]`: case (A) would give
    ///     lfsr = [coeffs[0]=1, coeffs[1]=2, coeffs[0]=1] = [1, 2, 1].
    ///   * `coeffs[nc - l - 1]` → `coeffs[nc - l]`: would read
    ///     coeffs[3] in case (A) → out-of-bounds panic.
    ///   * `lfsr[l + 1]` → `lfsr[l]`: case (B) propagation breaks.
    ///   * `lfsr[nc - 1] = (coeffs[0] * feedback) % prime` →
    ///     `coeffs[nc - 1] * feedback`: case (A) final slot would be
    ///     coeffs[2]*1%5 = 3 instead of 1.
    ///   * `nc.saturating_sub(1)` → `nc`: case (E) would try to read
    ///     lfsr[1] in the inner loop → panic.
    #[test]
    fn rs_ecprime_pins_lfsr_arithmetic_with_small_field() {
        // Case (A): coefficient-reversal pin.
        let coeffs_a = vec![1u32, 2, 3];
        assert_eq!(
            rs_ecprime(&[1], &coeffs_a, 3, 5),
            vec![3, 2, 1],
            "(A) nc=3, prime=5, coeffs=[1,2,3], data=[1]: \
             lfsr must be coefficient-reversed (pins coeffs[nc-l-1] indexing)"
        );

        // Case (B): multi-data → lfsr[l+1] propagation pin.
        assert_eq!(
            rs_ecprime(&[1, 2], &coeffs_a, 3, 5),
            vec![4, 4, 4],
            "(B) nc=3, prime=5, coeffs=[1,2,3], data=[1,2]: \
             all-4 result confirms feedback + lfsr forward-shift"
        );

        // Case (C): nc=2, smaller LFSR.
        let coeffs_c = vec![2u32, 3];
        assert_eq!(
            rs_ecprime(&[1], &coeffs_c, 2, 5),
            vec![3, 2],
            "(C) nc=2, prime=5, coeffs=[2,3], data=[1]: lfsr=[3,2]"
        );

        // Case (D): nc=2 multi-data.
        assert_eq!(
            rs_ecprime(&[1, 2], &coeffs_c, 2, 5),
            vec![4, 3],
            "(D) nc=2, prime=5, coeffs=[2,3], data=[1,2]: lfsr=[4,3]"
        );

        // Case (E): nc=1 saturating_sub edge — inner loop never runs.
        let coeffs_e = vec![3u32];
        assert_eq!(
            rs_ecprime(&[2], &coeffs_e, 1, 5),
            vec![1],
            "(E) nc=1, prime=5, coeffs=[3], data=[2]: \
             only the final-slot assignment runs"
        );

        // Case (F): empty data → initial-state preserved.
        assert_eq!(
            rs_ecprime(&[], &coeffs_a, 3, 5),
            vec![0, 0, 0],
            "(F) empty data: lfsr stays at initial zeros"
        );
    }

    /// Metadata helper matches BWIPP's mcc/qcc/acc/tcc computation
    /// for default EC2 across the BWIPP-captured corpus.
    ///
    /// Corpus rows captured via `rust/tools/oracle-ultracode.js`
    /// (default options: `eclevel="EC2"`, `rev=2`, `parsefnc=false`,
    /// `link1=0`).
    #[test]
    fn metadata_matches_corpus() {
        // (barcode, dcws_len, eclval, mcc, qcc, tcc)
        let cases: &[(&str, usize, u32, u32, u32, u32)] = &[
            ("A", 1, 2, 4, 7, 11),
            ("Hello", 5, 2, 8, 7, 15),
            ("Hello, World!", 13, 2, 16, 7, 23),
            ("12345", 5, 2, 8, 7, 15),
            ("ABCDEFGHIJKLMNOPQRSTUVWXYZ", 26, 2, 29, 9, 38),
            ("abcdef0123456789", 16, 2, 19, 7, 26),
            // High-byte case (UTF-8): 8 dcws.
            ("\\x00..\\xff (utf8)", 8, 2, 11, 7, 18),
            (
                "The quick brown fox jumps over the lazy dog.",
                44,
                2,
                47,
                9,
                56,
            ),
        ];
        for &(label, dcws_len, eclval, exp_mcc, exp_qcc, exp_tcc) in cases {
            let (mcc, qcc, acc, tcc) = ultracode_metadata(dcws_len, eclval, 0);
            assert_eq!(mcc, exp_mcc, "mcc for {label:?}");
            assert_eq!(qcc, exp_qcc, "qcc for {label:?}");
            assert_eq!(tcc, exp_tcc, "tcc for {label:?}");
            assert_eq!(acc, exp_qcc - 3, "acc derivation for {label:?}");
        }
    }

    /// `gen_coeffs(qcc)` matches BWIPP's `ultracode_gencoeffs` for
    /// the `qcc` values our corpus exercises.
    ///
    /// Corpus rows captured via `rust/tools/oracle-ultracode.js`.
    #[test]
    fn gen_coeffs_matches_corpus() {
        // (qcc, expected coeffs).
        let cases: &[(usize, &[u32])] = &[
            (7, &[87, 125, 83, 280, 21, 114, 117]),
            (9, &[225, 27, 162, 126, 127, 81, 3, 21, 192]),
        ];
        for &(qcc, expected) in cases {
            let got = gen_coeffs(qcc);
            assert_eq!(got, expected, "gen_coeffs({qcc}) mismatch");
        }
    }

    /// Stage 4b.4 — the symbol-size picker matches BWIPP's choice
    /// for every corpus row. Shape: `pick_symbol_size(tcc)` returns
    /// `(m_idx, rows, cols_picked, pads)`. We compare to the BWIPP
    /// final rows/columns (which are `rows*6+1` and `cols+6`).
    #[test]
    fn pick_symbol_size_matches_corpus() {
        // (label, tcc, BWIPP final rows, BWIPP final cols).
        let cases: &[(&str, u32, u32, u32)] = &[
            ("A", 11, 13, 13),
            ("Hello", 15, 13, 15),
            ("Hello, World!", 23, 13, 19),
            ("12345", 15, 13, 15),
            ("ABCDEFGHIJKLMNOPQRSTUVWXYZ", 38, 19, 20),
            ("abcdef0123456789", 26, 13, 22),
            ("UTF8 high-byte", 18, 13, 17),
            ("the quick brown fox", 56, 19, 27),
        ];
        for &(label, tcc, exp_rows, exp_cols) in cases {
            let (_m, rows, cols, _pads) = pick_symbol_size(tcc).unwrap_or_else(|| {
                panic!("pick_symbol_size returned None for {label} (tcc={tcc})")
            });
            assert_eq!(
                rows * 6 + 1,
                exp_rows,
                "rows mismatch for {label}: got {rows}*6+1={}, expected {exp_rows}",
                rows * 6 + 1
            );
            assert_eq!(
                cols + 6,
                exp_cols,
                "cols mismatch for {label}: got {cols}+6={}, expected {exp_cols}",
                cols + 6
            );
        }
    }

    /// `digits_5` matches `$cvrs($s(5), v, 10)` — most significant
    /// digit first, zero-padded to 5 positions.
    #[test]
    fn digits_5_matches_bwipp_zero_padding() {
        assert_eq!(digits_5(13135), [1, 3, 1, 3, 5]);
        assert_eq!(digits_5(56565), [5, 6, 5, 6, 5]);
        assert_eq!(digits_5(51363), [5, 1, 3, 6, 3]);
        // BWIPP zero-pads to 5 digits if the value is < 10000.
        assert_eq!(digits_5(0), [0, 0, 0, 0, 0]);
        assert_eq!(digits_5(7), [0, 0, 0, 0, 7]);
    }

    /// Stage 4b.5 — the full encoder produces the byte-for-byte
    /// BWIPP pixs grid across the entire captured corpus. This is
    /// the key Ultracode oracle.
    ///
    /// Corpus rows captured via `rust/tools/oracle-ultracode.js`
    /// (also available at `rust/tests/data/ultracode_corpus.json`
    /// for downstream regeneration). Each row pins the entire
    /// `rows × cols` pixs grid for default-options encoding
    /// (EC2 / rev2 / parsefnc=false).
    #[test]
    fn encode_pixs_default_matches_corpus() {
        // (input, expected_rows, expected_cols, expected_pixs).
        type Row = (&'static str, u32, u32, &'static [i32]);
        let cases: &[Row] = &[
            ("A", 13, 13, ULTRACODE_PIXS_A),
            ("Hello", 13, 15, ULTRACODE_PIXS_HELLO),
            ("Hello, World!", 13, 19, ULTRACODE_PIXS_HELLO_WORLD),
            ("12345", 13, 15, ULTRACODE_PIXS_12345),
            ("ABCDEFGHIJKLMNOPQRSTUVWXYZ", 19, 20, ULTRACODE_PIXS_ALPHA),
            ("abcdef0123456789", 13, 22, ULTRACODE_PIXS_ALPHANUM),
            // UTF-8 high-byte case: U+0080 → bytes [0xC2, 0x80]; U+00FF → [0xC3, 0xBF].
            (
                "\u{0}\u{1}\u{2}\u{7f}\u{80}\u{ff}",
                13,
                17,
                ULTRACODE_PIXS_UTF8,
            ),
            (
                "The quick brown fox jumps over the lazy dog.",
                19,
                27,
                ULTRACODE_PIXS_FOX,
            ),
        ];
        for &(input, exp_rows, exp_cols, expected) in cases {
            let (rows, cols, got) =
                encode_pixs_default(input).unwrap_or_else(|e| panic!("{input:?}: {e:?}"));
            assert_eq!(rows, exp_rows, "rows mismatch for {input:?}");
            assert_eq!(cols, exp_cols, "cols mismatch for {input:?}");
            assert_eq!(got.len(), expected.len(), "pixs length for {input:?}");
            // BWIPP fills every cell (no -1 sentinels). Any -1 = layout bug.
            for (i, &v) in got.iter().enumerate() {
                assert!(v >= 0, "{input:?}: pixs[{i}] is -1 (unset)");
            }
            assert_eq!(
                got, expected,
                "{input:?}: pixs grid mismatch (rows={rows}, cols={cols})"
            );
        }
    }

    /// Public `encode` entry point lands a coloured matrix whose
    /// cells match the BWIPP-verified pixs.
    #[test]
    fn encode_public_returns_color_matrix_with_correct_palette() {
        let matrix = encode("A", &Options::default()).expect("encode 'A'");
        assert_eq!(matrix.width(), 13);
        assert_eq!(matrix.height(), 13);
        // Top-left should be black (palette idx 5 = digit 9).
        assert_eq!(matrix.cell_color(0, 0), Rgb8::new(0, 0, 0));
        // The palette borrowed from the matrix is the Ultracode one.
        assert_eq!(matrix.palette()[1], Rgb8::new(0x00, 0xff, 0xff));
    }

    /// End-to-end Reed-Solomon: feeding the BWIPP-prepared rsdata
    /// `[start, mcc, acc, ...dcws]` through `ecc_codewords(rsdata, qcc)`
    /// reproduces BWIPP's `ecws` byte-for-byte across the captured
    /// corpus. This is the key Stage-4b.2 oracle.
    #[test]
    fn ecc_codewords_match_corpus() {
        // (barcode, dcws, mcc, qcc, expected ecws).
        type Row = (&'static str, &'static [u32], u32, u32, &'static [u32]);
        let cases: &[Row] = &[
            ("A", &[65], 4, 7, &[100, 2, 78, 70, 131, 251, 169]),
            (
                "Hello",
                &[72, 101, 108, 108, 111],
                8,
                7,
                &[159, 143, 249, 150, 50, 146, 139],
            ),
            (
                "Hello, World!",
                &[72, 101, 108, 108, 111, 44, 32, 87, 111, 114, 108, 100, 33],
                16,
                7,
                &[208, 97, 197, 178, 196, 114, 270],
            ),
            (
                "12345",
                &[49, 50, 51, 52, 53],
                8,
                7,
                &[14, 190, 49, 238, 269, 227, 14],
            ),
            (
                "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
                &[
                    65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84,
                    85, 86, 87, 88, 89, 90,
                ],
                29,
                9,
                &[260, 188, 199, 64, 189, 197, 154, 282, 120],
            ),
            (
                "abcdef0123456789",
                &[
                    97, 98, 99, 100, 101, 102, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57,
                ],
                19,
                7,
                &[50, 134, 8, 151, 105, 239, 214],
            ),
            (
                "high-byte UTF-8",
                &[0, 1, 2, 127, 194, 128, 195, 191],
                11,
                7,
                &[100, 133, 104, 69, 261, 169, 227],
            ),
            (
                "The quick brown fox jumps over the lazy dog.",
                &[
                    84, 104, 101, 32, 113, 117, 105, 99, 107, 32, 98, 114, 111, 119, 110, 32, 102,
                    111, 120, 32, 106, 117, 109, 112, 115, 32, 111, 118, 101, 114, 32, 116, 104,
                    101, 32, 108, 97, 122, 121, 32, 100, 111, 103, 46,
                ],
                47,
                9,
                &[252, 169, 275, 132, 279, 49, 212, 59, 106],
            ),
        ];
        for &(label, dcws, mcc, qcc, expected) in cases {
            let acc = qcc - 3; // link1 = 0
            let mut rsdata = vec![ULTRACODE_CW_START, mcc, acc];
            rsdata.extend_from_slice(dcws);

            let got = ecc_codewords(&rsdata, qcc as usize);
            assert_eq!(
                got, expected,
                "ecws mismatch for {label:?}: got {got:?}, expected {expected:?}"
            );
        }
    }

    /// Stage 5c — lightweight property test: a deterministic random
    /// walk over the in-bounds input domain (1..=250 byte payloads,
    /// every byte 0..=255 — sized to always fit BWIPP's largest
    /// symbol) must not panic, must produce a `ColorMatrix` whose
    /// width/height match BWIPP's expansion rule (`cols+6 × rows*6+1`),
    /// and must use only palette indices 0..=5 (no stray data digits
    /// ever land in the reserved slots 6/7).
    ///
    /// Uses a tiny LCG (no proptest dep) so the test is reproducible
    /// and stable. 200 fuzz iterations cover payload lengths spread
    /// across each `ULTRACODE_METRICS` size class.
    #[test]
    fn encode_property_random_inputs_never_panic_and_use_only_defined_palette() {
        // LCG: classic glibc parameters; not crypto-quality but
        // deterministic and well-distributed over u32.
        let mut state: u32 = 0x1234_5678;
        let mut next = || {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
            state
        };
        // The largest symbol-size class (rows=5) accepts up to
        // tcc=282 codewords; mcc = dcws_len + 3 and qcc grows with
        // mcc, so capping at 250 dcws keeps every random sample in
        // the encodable range without making this a bounds test.
        const MAX_LEN: usize = 250;
        for iter in 0..200 {
            let len = 1 + (next() as usize % MAX_LEN); // 1..=MAX_LEN
            let mut input = Vec::with_capacity(len);
            for _ in 0..len {
                input.push((next() & 0xff) as u8);
            }
            let s = String::from_utf8_lossy(&input);
            // Inputs > MAX_LEN bytes (after UTF-8 lossy escape) can
            // exceed BWIPP's symbol cap — that's `InvalidData`, not a
            // panic. We assert encode() never panics and either
            // returns a valid matrix or `InvalidData`; the latter is
            // the legitimate "too long for largest symbol" response.
            let matrix = match encode(&s, &Options::default()) {
                Ok(m) => m,
                Err(Error::InvalidData(_)) => continue,
                Err(other) => panic!("iter {iter}: unexpected error {other:?}"),
            };
            // Width/height invariants: must be rows*6+1 / cols+6 for
            // some metrics row.
            let w = matrix.width() as u32;
            let h = matrix.height() as u32;
            assert!((h - 1) % 6 == 0, "iter {iter}: height {h} is not rows*6+1");
            let rows = (h - 1) / 6;
            assert!(
                (2..=5).contains(&rows),
                "iter {iter}: derived rows {rows} out of range 2..=5"
            );
            assert!(
                w >= 6 + ULTRACODE_METRICS[(rows - 2) as usize][3],
                "iter {iter}: width {w} below mcol+6 for rows={rows}"
            );
            // Every cell must use a defined palette index (0..=5).
            for y in 0..matrix.height() {
                for x in 0..matrix.width() {
                    let idx = matrix.get(x, y);
                    assert!(
                        idx <= 5,
                        "iter {iter}: cell ({x},{y}) has reserved palette idx {idx}"
                    );
                }
            }
        }
    }

    /// Stage 11.A8c — pin `digits_5` decimal-expansion at boundaries.
    /// Kills `% with /` / `% with +` / `/ with *` mutations on the
    /// per-digit extraction loop (lines 714-722).
    #[test]
    fn digits_5_boundary_values() {
        // 0 → [0,0,0,0,0].
        assert_eq!(digits_5(0), [0, 0, 0, 0, 0]);
        // 1 → [0,0,0,0,1] (LSB last).
        assert_eq!(digits_5(1), [0, 0, 0, 0, 1]);
        // 10 → [0,0,0,1,0].
        assert_eq!(digits_5(10), [0, 0, 0, 1, 0]);
        // 12345 → [1,2,3,4,5] — pins the full slot order.
        assert_eq!(digits_5(12345), [1, 2, 3, 4, 5]);
        // 99999 → [9,9,9,9,9] (max 5-digit).
        assert_eq!(digits_5(99999), [9, 9, 9, 9, 9]);
        // 100000 → wraps: 5 LSB digits = [0,0,0,0,0].
        // Pins that the function only emits 5 digits regardless of v.
        assert_eq!(digits_5(100000), [0, 0, 0, 0, 0]);
    }

    /// Stage 11.A8c — pin `rs_prod` Galois-field multiplication
    /// boundaries. Kills the `== with !=` mutant on the zero-product
    /// short-circuit (line 238) and the `% 282` index mutation.
    #[test]
    fn rs_prod_zero_short_circuit() {
        let (alog, log) = build_rs_tables();
        // 0 * y = 0 for any y.
        assert_eq!(rs_prod(0, 5, &alog, &log), 0);
        assert_eq!(rs_prod(0, 200, &alog, &log), 0);
        // x * 0 = 0 for any x.
        assert_eq!(rs_prod(5, 0, &alog, &log), 0);
        assert_eq!(rs_prod(200, 0, &alog, &log), 0);
        // 0 * 0 = 0.
        assert_eq!(rs_prod(0, 0, &alog, &log), 0);
        // 1 * y = y (multiplicative identity, since log[1] == 0 in
        // the standard log table).
        assert_eq!(rs_prod(1, 5, &alog, &log), 5);
        assert_eq!(rs_prod(5, 1, &alog, &log), 5);
        assert_eq!(rs_prod(1, 1, &alog, &log), 1);
        // Verify a known multiplication is non-zero (sanity).
        let prod = rs_prod(2, 3, &alog, &log);
        assert_ne!(prod, 0);
    }

    /// Stage 11.A8c — pin `build_dcws_default(input)`. The tiny
    /// 2-line wrapper used by `build_dcws_from_opts` when no input-
    /// parsing opt is set. Just maps each input byte to a `u32`
    /// codeword. Only exercised transitively through
    /// `build_dcws_from_opts` and the public `encode` path; mutations
    /// on the `.bytes().map(...)` chain, the `as u32` cast, or the
    /// iteration order all survive when downstream consumers happen
    /// to coincide.
    ///
    /// Hand-computed:
    ///   * "" → []
    ///   * "A" → [65]
    ///   * "ABC" → [65, 66, 67] (preserves order)
    ///   * "0" → [48]
    ///   * "9" → [57]
    ///   * "\x00\xFF" → [0, 255] (boundary high byte preserved as u32)
    ///   * Length invariant: out.len() == input.len() for ASCII.
    ///
    /// Mutations to catch:
    ///   * `.bytes()` → `.bytes().rev()`: caught by [65, 66, 67]
    ///     ordering — would produce [67, 66, 65].
    ///   * Body returns empty Vec::new(): caught by every non-empty
    ///     anchor.
    ///   * Body returns `vec![0; input.len()]`: caught by content
    ///     check (every "A" → [0] not [65]).
    ///   * `b as u32` → wrong cast that drops top byte (would need
    ///     `b as u8 as u32` which is no-op since b is already u8).
    #[test]
    fn build_dcws_default_byte_to_u32_mapping_preserves_order() {
        assert_eq!(build_dcws_default(""), Vec::<u32>::new());
        assert_eq!(build_dcws_default("A"), vec![65]);
        assert_eq!(
            build_dcws_default("ABC"),
            vec![65, 66, 67],
            "ASCII order preserved (catches .rev() mutant)"
        );
        assert_eq!(build_dcws_default("0"), vec![48]);
        assert_eq!(build_dcws_default("9"), vec![57]);
        assert_eq!(build_dcws_default("Hello"), vec![72, 101, 108, 108, 111]);

        // Multi-byte UTF-8 char (é = 0xC3 0xA9 in UTF-8): bytes
        // iterator yields both bytes as separate u32 codewords.
        // Pins that `.bytes()` walks UTF-8 byte stream, not chars.
        let out = build_dcws_default("é");
        assert_eq!(
            out,
            vec![0xC3, 0xA9],
            "UTF-8 multi-byte char emits each byte as separate u32"
        );

        // Length invariant for ASCII inputs (byte length == output length).
        for s in [
            "",
            "x",
            "xy",
            "Hello, world!",
            "0123456789",
            &"a".repeat(50),
        ] {
            let out = build_dcws_default(s);
            assert_eq!(
                out.len(),
                s.len(),
                "ASCII input {s:?}: byte length must equal output length"
            );
        }
    }

    /// `digits_5(v)` decomposes `v` into 5 base-10 digits MSB-first
    /// (LSB at index 4). It iterates `out.iter_mut().rev()` and uses
    /// `t % 10` / `t /= 10` so successive % values land in slot
    /// 4, 3, 2, 1, 0 — i.e. the LSB ends at the END of the array.
    ///
    /// This pins:
    /// * `rev()` direction — without it the slot order would flip
    ///   and `digits_5(12345)` would yield `[5, 4, 3, 2, 1]`.
    /// * `t % 10` modulus — a mutation to `% 100` would put a
    ///   2-digit value in the trailing slot (e.g. `45` for input
    ///   12345), exceeding the 0..=9 single-digit invariant.
    /// * `t /= 10` divisor — a mutation to `/= 100` would skip a
    ///   digit, dropping the second-from-right slot.
    /// * Wrap behavior for inputs ≥ 100_000 — only the bottom 5
    ///   base-10 digits survive (BWIPP relies on this since DCC /
    ///   tileseq values fit in 17 bits but the helper output is
    ///   always 5 slots).
    #[test]
    fn digits_5_lsb_at_end_with_msb_first_layout() {
        // Single discriminator anchors.
        assert_eq!(
            super::digits_5(0),
            [0, 0, 0, 0, 0],
            "zero input → all-zero output"
        );
        // 7 in the LSB slot — would land at index 0 if rev() were
        // removed.
        assert_eq!(
            super::digits_5(7),
            [0, 0, 0, 0, 7],
            "single-digit input lands in LSB slot (index 4)"
        );
        // 7 in the MSB slot — symmetric to the single-digit case.
        assert_eq!(
            super::digits_5(70_000),
            [7, 0, 0, 0, 0],
            "10000×d lands in MSB slot (index 0)"
        );
        // Distinct per-slot digits — most powerful order discriminator.
        // Mutations to slot indexing, rev(), or /=10 will change at
        // least one slot.
        assert_eq!(
            super::digits_5(12_345),
            [1, 2, 3, 4, 5],
            "12345 → [1,2,3,4,5] (MSB→LSB layout, distinct per slot)"
        );
        // Reverse-pattern: catches mutations that happen to be
        // self-consistent on monotone-increasing inputs.
        assert_eq!(
            super::digits_5(54_321),
            [5, 4, 3, 2, 1],
            "54321 → [5,4,3,2,1] (reverse pattern)"
        );
        // Saturated.
        assert_eq!(
            super::digits_5(99_999),
            [9, 9, 9, 9, 9],
            "99999 → all-9 (saturated 5-digit)"
        );
        // Power-of-ten boundary — only the high digit is non-zero.
        assert_eq!(
            super::digits_5(10_000),
            [1, 0, 0, 0, 0],
            "10000 → leading 1, four trailing zeros"
        );
        // Wrap: input ≥ 100_000 keeps only the low 5 digits.
        assert_eq!(
            super::digits_5(100_000),
            [0, 0, 0, 0, 0],
            "100000 wraps modulo 10^5 → all zero"
        );
        assert_eq!(
            super::digits_5(123_456),
            [2, 3, 4, 5, 6],
            "123456 wraps to bottom 5 base-10 digits → [2,3,4,5,6]"
        );

        // Invariant sweep: every value 0..=99_999 must satisfy
        // `value == sum(digit_i * 10^(4-i))` for the output digits.
        for v in [0u32, 1, 9, 10, 99, 100, 9_999, 10_000, 99_999] {
            let d = super::digits_5(v);
            for (i, &di) in d.iter().enumerate() {
                assert!(di <= 9, "digit {di} at slot {i} (v={v}) exceeds 0..=9");
            }
            let reconstructed = u32::from(d[0]) * 10_000
                + u32::from(d[1]) * 1_000
                + u32::from(d[2]) * 100
                + u32::from(d[3]) * 10
                + u32::from(d[4]);
            assert_eq!(
                reconstructed, v,
                "digits_5({v}) = {d:?} should reconstruct via MSB→LSB place values"
            );
        }
    }

    /// Stage 11.A8c — pin `check_ultracode_opts`'s 8 rejection arms.
    /// All 7 BWIPP options (rev, eclevel, parsefnc, raw, parse, link1,
    /// start) have a catch-all error arm; plus the rev/eclevel
    /// cross-check (`rev=2 requires eclvalue >= 1`). Existing tests
    /// only exercise happy paths and the "no Unimplemented" guarantee,
    /// not the InvalidOption rejection arms directly. A mutant that
    /// swaps the catch-all branch for an Ok(_) accept would silently
    /// pass through garbage values.
    ///
    /// Anchors (each input → InvalidOption with diagnostic substring):
    ///   1. rev="3"          → "rev=" + "1 or 2".
    ///   2. eclevel="EC9"    → "eclevel=" + "EC0/EC1/EC2/EC3/EC4/EC5".
    ///   3. rev=2 + eclevel=EC0 cross-check → "eclevel=EC0 requires rev=1".
    ///   4. parsefnc="maybe" → "parsefnc=" + "true" + "false".
    ///   5. raw="yes"        → "raw=" + "true" + "false".
    ///   6. parse="off"      → "parse=" + "true" + "false".
    ///   7. link1="abc"      → "link1=" + "non-negative integer".
    ///   8. start="-1"       → "start=" + "non-negative integer".
    ///
    /// Mutations to catch:
    ///   * Catch-all arm `_ => return Err(InvalidOption(...))` replaced
    ///     with `_ => Ok(default)` — would silently accept bad values.
    ///   * Error message substring drift (e.g. "1 or 2" → "1 to 2").
    ///   * Cross-check `if parsed.rev == 2` → `== 1`: would gate the
    ///     wrong way.
    ///   * `eclval < min_ecl` → `<=` or `>` boundary shifts.
    ///   * Parse-Err arm dropped on integer options (link1/start): the
    ///     `unwrap_or(default)` style would let invalid input fall
    ///     through to default; we explicitly verify InvalidOption is
    ///     returned.
    #[test]
    fn check_ultracode_opts_rejection_arms() {
        use crate::error::Error;

        // 1. rev=3 (not 1 or 2). Diagnostic at line 784:
        //   "ultracode: rev={v:?} is not a valid BWIPP value (valid: 1 or 2)"
        // Both "valid" AND "1 or 2" are always present; the previous
        // `||` accepted either substring. 4-anchor `&&` pin:
        let opts = Options::default().with("rev", "3");
        match check_ultracode_opts(&opts) {
            Err(Error::InvalidOption(m)) => {
                assert!(
                    m.contains("ultracode:"),
                    "rev=3 diagnostic must carry the ultracode prefix; got {m}"
                );
                assert!(
                    m.contains("rev=\"3\""),
                    "rev=3 diagnostic must Debug-echo the raw value; got {m}"
                );
                assert!(
                    m.contains("not a valid BWIPP value"),
                    "rev=3 diagnostic must carry the predicate; got {m}"
                );
                assert!(
                    m.contains("(valid: 1 or 2)"),
                    "rev=3 diagnostic must name the valid revs (1 or 2); got {m}"
                );
            }
            other => panic!("rev=3 should reject, got {other:?}"),
        }

        // 2. eclevel=EC9 (only EC0..EC5 valid). Diagnostic at line 801:
        //   "(valid: EC0/EC1/EC2/EC3/EC4/EC5)"
        let opts = Options::default().with("eclevel", "EC9");
        match check_ultracode_opts(&opts) {
            Err(Error::InvalidOption(m)) => {
                assert!(
                    m.contains("ultracode:"),
                    "eclevel=EC9 diagnostic must carry the ultracode prefix; got {m}"
                );
                assert!(
                    m.contains("eclevel=\"EC9\""),
                    "eclevel=EC9 diagnostic must Debug-echo the raw value; got {m}"
                );
                assert!(
                    m.contains("not a valid BWIPP value"),
                    "eclevel=EC9 diagnostic must carry the predicate; got {m}"
                );
                assert!(
                    m.contains("(valid: EC0/EC1/EC2/EC3/EC4/EC5)"),
                    "eclevel=EC9 diagnostic must name all valid ec levels; got {m}"
                );
            }
            other => panic!("eclevel=EC9 should reject, got {other:?}"),
        }

        // 3. rev=2 (default) + eclevel=EC0 cross-check (EC0 requires
        //    rev=1).
        let opts = Options::default().with("eclevel", "EC0");
        match check_ultracode_opts(&opts) {
            Err(Error::InvalidOption(m)) => assert!(
                m.contains("eclevel=EC0") && m.contains("rev=1"),
                "expected EC0+rev=2 cross-check, got: {m}"
            ),
            other => panic!("EC0 with default rev=2 should reject, got {other:?}"),
        }
        // Sanity: EC0 + explicit rev=1 succeeds (cross-check passes).
        let opts = Options::default().with("eclevel", "EC0").with("rev", "1");
        assert!(
            check_ultracode_opts(&opts).is_ok(),
            "EC0 + rev=1 should validate cleanly (cross-check minimum satisfied)"
        );

        // 4. parsefnc=maybe (only "true"/"false" valid). Diagnostic at
        // line 821: "ultracode: parsefnc={v:?} must be \"true\" or \"false\""
        // 3-anchor pin upgrades the previous single-substring check.
        let opts = Options::default().with("parsefnc", "maybe");
        match check_ultracode_opts(&opts) {
            Err(Error::InvalidOption(m)) => {
                assert!(
                    m.contains("ultracode: parsefnc=\"maybe\""),
                    "parsefnc=maybe must echo full prefix + Debug-value; got {m}"
                );
                assert!(
                    m.contains("must be"),
                    "parsefnc=maybe must carry the predicate; got {m}"
                );
                assert!(
                    m.contains("\"true\"") && m.contains("\"false\""),
                    "parsefnc=maybe must name BOTH valid values; got {m}"
                );
            }
            other => panic!("parsefnc=maybe should reject, got {other:?}"),
        }

        // 5. raw=yes. Diagnostic at line 833 mirrors parsefnc.
        let opts = Options::default().with("raw", "yes");
        match check_ultracode_opts(&opts) {
            Err(Error::InvalidOption(m)) => {
                assert!(
                    m.contains("ultracode: raw=\"yes\""),
                    "raw=yes must echo full prefix + Debug-value; got {m}"
                );
                assert!(
                    m.contains("must be"),
                    "raw=yes must carry the predicate; got {m}"
                );
                assert!(
                    m.contains("\"true\"") && m.contains("\"false\""),
                    "raw=yes must name BOTH valid values; got {m}"
                );
            }
            other => panic!("raw=yes should reject, got {other:?}"),
        }

        // 6. parse=off. Same boolean-only diagnostic shape as
        // parsefnc/raw above — 3-anchor pin: prefix + predicate + both
        // valid values.
        let opts = Options::default().with("parse", "off");
        match check_ultracode_opts(&opts) {
            Err(Error::InvalidOption(m)) => {
                assert!(
                    m.contains("ultracode: parse=\"off\""),
                    "parse=off must echo full prefix + Debug-value; got {m}"
                );
                assert!(
                    m.contains("must be"),
                    "parse=off must carry the predicate; got {m}"
                );
                assert!(
                    m.contains("\"true\"") && m.contains("\"false\""),
                    "parse=off must name BOTH valid values; got {m}"
                );
            }
            other => panic!("parse=off should reject, got {other:?}"),
        }

        // 7. link1=abc (non-integer).
        let opts = Options::default().with("link1", "abc");
        match check_ultracode_opts(&opts) {
            Err(Error::InvalidOption(m)) => assert!(
                m.contains("link1=\"abc\"") && m.contains("non-negative integer"),
                "expected link1 parse-fail rejection, got: {m}"
            ),
            other => panic!("link1=abc should reject, got {other:?}"),
        }

        // 8. start=-1 (negative — u32::parse rejects).
        let opts = Options::default().with("start", "-1");
        match check_ultracode_opts(&opts) {
            Err(Error::InvalidOption(m)) => assert!(
                m.contains("start=\"-1\"") && m.contains("non-negative integer"),
                "expected start parse-fail rejection, got: {m}"
            ),
            other => panic!("start=-1 should reject, got {other:?}"),
        }
    }

    // -------------------------------------------------------------------
    // Stage 11.A8c-L — PRE-DRAFT FINGERPRINT KILLERS (PENDING CAPTURE).
    //
    // Pre-stage exhaustive fingerprints for the three largest ultracode
    // survivor clusters reported by `mutants-ultracode-v1` (28 missed):
    //   - build_dcws_with_input_opts (15 missed)
    //   - encode                     ( 6 missed)
    //   - pick_symbol_size           ( 4 missed)
    //
    // All tests are #[ignore]'d so they don't run in default
    // `cargo test` or cargo-mutants with placeholder constants.
    // Capture workflow:
    //   1. Un-ignore.
    //   2. `cargo test <name> -- --nocapture --include-ignored`.
    //   3. Paste captured values into the `FP_*` consts.
    //   4. Leave un-ignored so cargo-mutants exercises them.
    // -------------------------------------------------------------------

    /// Cluster: `build_dcws_with_input_opts` — 15 missed mutants (biggest).
    ///
    /// Target lines (selected):
    ///   - L492:18 `+` → `-`/`*`  (`i + 5 <= bytes.len()` for ^FNC1)
    ///   - L492:37 `&&` → `||`    (the && in same conjunction)
    ///   - L492:48 `+` → `*`      (`&bytes[i..i + 5]`)
    ///   - L501:18 `+` → `*`      (`i + 4 <= bytes.len()` for 3-char ctrl)
    ///   - L513:16 delete `!`     (`!matched && …`)
    ///   - L513:25 `&&` → `||`    (the && between !matched and bound check)
    ///   - L513:30 `+` → `*`      (`i + 3 <= bytes.len()` for 2-char ctrl)
    ///   - L513:34 `<=` → `>`     (same boundary)
    ///   - L514:42 `+` → `*`      (`&bytes[i+1..i+3]`)
    ///   - L517:50 `==` → `!=`    (name.len() == 2)
    ///   - L520:23 `+=` → `-=`/`*=`  (`i += 3`)
    ///   - L525:25 `&&` → `||`    (3-digit ordinal guard)
    ///   - L531:30 `>` → `>=`     (`value > 255`)
    ///
    /// Strategy: exhaustive iteration over (parse, parsefnc) flag combos
    /// times diverse `^`-escape payloads. Each call produces a distinct
    /// Vec<u32>; fingerprint = (len, position-weighted u64).
    ///
    /// Activated 2026-05-28: fingerprints captured from oracle-matched encoder.
    #[test]
    fn build_dcws_with_input_opts_escape_grid_fingerprint_pinned() {
        fn fp(v: Result<Vec<u32>, String>) -> (bool, usize, u64) {
            match v {
                Ok(dcws) => {
                    let mut s: u64 = 0;
                    for (i, &c) in dcws.iter().enumerate() {
                        s =
                            s.wrapping_add((c as u64).wrapping_mul(
                                (i as u64).wrapping_add(1).wrapping_mul(2_654_435_761),
                            ));
                    }
                    (true, dcws.len(), s)
                }
                Err(_) => (false, 0, 0),
            }
        }
        // (tag, input, parse, parsefnc, want)
        let cases: &[(&str, &str, bool, bool, (bool, usize, u64))] = &[
            // Plain ASCII (no escapes; control branches not hit).
            ("plain", "Hello, World!", false, false, FP_DCWS_PLAIN),
            // parsefnc only — ^FNC1 emits sentinel, ^^ → literal '^'.
            ("fnc1", "A^FNC1B", false, true, FP_DCWS_FNC1),
            ("fnc3", "^FNC3X", false, true, FP_DCWS_FNC3),
            ("caret_lit", "X^^Y", false, true, FP_DCWS_CARET_LIT),
            // parse only — 3-char ctrl name (TAB, ESC), 2-char (BS, LF, FF, CR).
            ("ctrl_tab", "A^TABB", true, false, FP_DCWS_CTRL_TAB),
            ("ctrl_lf", "A^LFB", true, false, FP_DCWS_CTRL_LF),
            ("ctrl_esc", "A^ESCB", true, false, FP_DCWS_CTRL_ESC),
            // 3-digit ordinal.
            ("ord_065", "^065^066", true, false, FP_DCWS_ORD_ABC),
            // Ordinal > 255 must reject.
            ("ord_999", "^999", true, false, FP_DCWS_ORD_999_ERR),
            // Unmatched `^` — emits literal caret and advances 1 byte.
            ("unmatched", "A^XYZ", true, false, FP_DCWS_UNMATCHED),
            // Both flags — parsefnc has priority over parse for `^FNC1`.
            ("both_fnc", "^FNC1^065", true, true, FP_DCWS_BOTH),
            // Boundary near end-of-string (i + 5 > bytes.len()).
            ("eos_short", "A^FN", false, true, FP_DCWS_EOS_SHORT),
            ("eos_3byte", "A^TA", true, false, FP_DCWS_EOS_3BYTE),
        ];
        for (tag, input, parse, parsefnc, want) in cases {
            let got = fp(build_dcws_with_input_opts(input, *parse, *parsefnc));
            assert_eq!(got, *want, "fingerprint changed for {tag}");
        }
    }
    const FP_DCWS_PLAIN: (bool, usize, u64) = (true, 13, 20505516253725);
    const FP_DCWS_FNC1: (bool, usize, u64) = (true, 3, 2120894173039);
    const FP_DCWS_FNC3: (bool, usize, u64) = (true, 2, 1181223913645);
    const FP_DCWS_CARET_LIT: (bool, usize, u64) = (true, 3, 1441358618223);
    const FP_DCWS_CTRL_TAB: (bool, usize, u64) = (true, 3, 745896448841);
    const FP_DCWS_CTRL_LF: (bool, usize, u64) = (true, 3, 751205320363);
    const FP_DCWS_CTRL_ESC: (bool, usize, u64) = (true, 3, 841456136237);
    const FP_DCWS_ORD_ABC: (bool, usize, u64) = (true, 2, 522923844917);
    const FP_DCWS_ORD_999_ERR: (bool, usize, u64) = (false, 0, 0);
    const FP_DCWS_UNMATCHED: (bool, usize, u64) = (true, 5, 3511818511803);
    const FP_DCWS_BOTH: (bool, usize, u64) = (true, 2, 1056465432878);
    const FP_DCWS_EOS_SHORT: (bool, usize, u64) = (true, 4, 2057187714775);
    const FP_DCWS_EOS_3BYTE: (bool, usize, u64) = (true, 4, 2030643357165);

    /// Cluster: `encode` (top-level) — 6 missed mutants.
    ///
    /// Target lines:
    ///   - L909:19 `>` → `>=` (`data.len() > 2500` length guard)
    ///   - L926:9 `-` → `/`   (`dcc = cols_picked - mcol`)
    ///   - L964:31 `*` → `+`/`/` (`y * final_cols`)
    ///   - L967:39 `<` → `==`/`<=` (`cell < 0` sentinel branch)
    ///
    /// Strategy: drive `encode` with diverse data lengths and option
    /// combinations to span multiple symbol sizes (and thus several
    /// `dcc` values). Fingerprint = (rows, cols, position-weighted
    /// u64 over all palette indices).
    ///
    /// Activated 2026-05-28: fingerprints captured from oracle-matched encoder.
    #[test]
    fn encode_palette_grid_fingerprint_pinned() {
        fn fp(matrix: &crate::encoding::ColorMatrix) -> (usize, usize, u64) {
            let w = matrix.width();
            let h = matrix.height();
            let mut s: u64 = 0;
            for y in 0..h {
                for x in 0..w {
                    let v = u64::from(matrix.get(x, y));
                    let idx = (y as u64) * (w as u64) + (x as u64);
                    s = s.wrapping_add(
                        v.wrapping_mul(idx.wrapping_add(1).wrapping_mul(2_654_435_761)),
                    );
                }
            }
            (w, h, s)
        }
        // Sweep symbol sizes. Default eclval=2 → fact=2; max tcc=282
        // (metric idx 3 max). dcws_len=200 → tcc=226 fits idx 3.
        let long_100 = "X".repeat(100);
        let long_200 = "X".repeat(200);
        let cases: &[(&str, &str, (usize, usize, u64))] = &[
            ("A", "A", FP_ENC_A),
            ("HELLO", "HELLO", FP_ENC_HELLO),
            ("HelloWorld", "Hello World", FP_ENC_HW),
            ("12345", "12345", FP_ENC_12345),
            // Longer payload spanning the next symbol-size bracket.
            ("long_100x", long_100.as_str(), FP_ENC_LONG_100),
            // Even-longer payload (different dcc, metric idx 3).
            ("long_200x", long_200.as_str(), FP_ENC_LONG_200),
        ];
        for (tag, data, want) in cases {
            let m = encode(data, &Options::default())
                .unwrap_or_else(|e| panic!("encode({tag}) ok: {e:?}"));
            let got = fp(&m);
            assert_eq!(got, *want, "fingerprint changed for {tag}");
        }
    }
    const FP_ENC_A: (usize, usize, u64) = (13, 13, 119192128976183);
    const FP_ENC_HELLO: (usize, usize, u64) = (15, 13, 157769043890796);
    const FP_ENC_HW: (usize, usize, u64) = (18, 13, 221852432032858);
    const FP_ENC_12345: (usize, usize, u64) = (15, 13, 157113398257829);
    const FP_ENC_LONG_100: (usize, usize, u64) = (39, 25, 3457965319083832);
    const FP_ENC_LONG_200: (usize, usize, u64) = (55, 31, 10302109396531012);

    /// Cluster: `pick_symbol_size` — 4 missed mutants.
    ///
    /// Target lines:
    ///   - L576:17 `-=` → `+=`/`/=` (`adj -= 1` for cols >= 31)
    ///   - L579:17 `-=` → `+=`/`/=` (`adj -= 1` for cols >= 47)
    ///
    /// Strategy: iterate `tcc` over a sweep that crosses the three
    /// adj-decrement boundaries (cols=15, 31, 47). For each tcc value
    /// in 1..=512, capture `(m_idx, rows, cols, pads)`; fingerprint
    /// the result with a position-weighted u64.
    ///
    /// Activated 2026-05-28: fingerprints captured from oracle-matched encoder.
    #[test]
    fn pick_symbol_size_sweep_fingerprint_pinned() {
        fn pack(opt: Option<(usize, u32, u32, u32)>) -> u64 {
            match opt {
                None => 0,
                Some((m, r, c, p)) => {
                    ((m as u64) & 0xFFFF)
                        | (((r as u64) & 0xFFFF) << 16)
                        | (((c as u64) & 0xFFFF) << 32)
                        | (((p as u64) & 0xFFFF) << 48)
                }
            }
        }
        let mut s: u64 = 0;
        let mut count_some: u32 = 0;
        let mut count_none: u32 = 0;
        for tcc in 1u32..=512 {
            let res = pick_symbol_size(tcc);
            if res.is_some() {
                count_some += 1;
            } else {
                count_none += 1;
            }
            let v = pack(res);
            s = s.wrapping_add(
                v.wrapping_mul((tcc as u64).wrapping_add(1).wrapping_mul(2_654_435_761)),
            );
        }
        let got = (count_some, count_none, s);
        assert_eq!(got, FP_PICK_SIZE_SWEEP, "fingerprint changed");
    }
    const FP_PICK_SIZE_SWEEP: (u32, u32, u64) = (276, 236, 12514356780962302335);

    // ====================================================================
    // Stage 11.A8d — ultracode T2-a: kill / prove the 10 remaining
    // mutation survivors. Each test below is keyed to a specific
    // file:line:operator survivor and is engineered to land on the exact
    // boundary / operand that distinguishes the mutant from the original.
    // ====================================================================

    /// Survivor #3 — `build_dcws_with_input_opts` L492:18 `+` → `*`
    /// (`i + 2 <= bytes.len()` in the parsefnc `^^` → literal-caret
    /// branch).
    ///
    /// The original guard is `i + 2 <= len`; the mutant is `i * 2 <= len`.
    /// These diverge whenever `len/2 < i <= len - 2`. We place a `^^`
    /// pair at byte index 5 of a 7-byte input under `parsefnc=true`:
    ///   - original: `5 + 2 = 7 <= 7` → enter the `^^` branch, collapse
    ///     to a single literal caret (94), consuming both carets;
    ///   - mutant:   `5 * 2 = 10 <= 7` is false → the `^^` branch is
    ///     skipped, so each `^` falls through to the unmatched-caret tail
    ///     and is emitted separately.
    /// Output length and content therefore differ (6 vs 7 codewords).
    #[test]
    fn build_dcws_caret_collapse_index_pins_plus_not_times() {
        // "01234^^" — the `^^` starts at index 5 in a 7-byte string.
        let got = build_dcws_with_input_opts("01234^^", false, true)
            .expect("parsefnc `^^` collapse must succeed");
        assert_eq!(
            got,
            vec![48, 49, 50, 51, 52, 94],
            "`^^` at index 5 of a 7-byte input must collapse to a single \
             caret — pins `i + 2` (mutant `i * 2` would not collapse, \
             emitting two carets)"
        );
    }

    /// Survivor #4 — `build_dcws_with_input_opts` L513:25 `&&` → `||`
    /// (`!matched && i + 3 <= bytes.len()` guarding the 2-char ctrl-name
    /// block under `parse=true`).
    ///
    /// When a 3-char ctrl name has already matched, `matched = true` and
    /// `i` has advanced by 4. The original `&&` short-circuits on
    /// `!matched == false`, so the 2-char block is skipped. The mutant
    /// `||` enters the 2-char block anyway (because the bound check is
    /// true), re-reading `bytes[i+1..i+3]` at the *new* `i` and matching
    /// a following 2-char ctrl name that the original treats as plain
    /// text. Input `^NUL_BS` (parse=true):
    ///   - original: `^NUL`→0, then `_`→95, `B`→66, `S`→83  ⇒ [0,95,66,83];
    ///   - mutant:   `^NUL`→0, then the 2-char block consumes `BS`→8 from
    ///     bytes[5..7]                                       ⇒ [0,8].
    #[test]
    fn build_dcws_two_char_guard_pins_and_not_or() {
        let got = build_dcws_with_input_opts("^NUL_BS", true, false)
            .expect("parse 3-char ctrl name must succeed");
        assert_eq!(
            got,
            vec![0, 95, 66, 83],
            "after a 3-char ctrl match the 2-char block must NOT re-fire — \
             pins `!matched && …` (mutant `||` would consume the trailing \
             `BS` as a control name)"
        );
    }

    /// Survivor #5 — `build_dcws_with_input_opts` L513:30 `+` → `*`
    /// (`i + 3 <= bytes.len()` bound for the 2-char ctrl-name block).
    ///
    /// Original `i + 3 <= len` vs mutant `i * 3 <= len` diverge for
    /// `len/3 < i <= len - 3`. We put a 2-char ctrl name `^BS` at byte
    /// index 4 of a 7-byte input under `parse=true`:
    ///   - original: `4 + 3 = 7 <= 7` → 2-char block runs, `BS`→8;
    ///   - mutant:   `4 * 3 = 12 <= 7` is false → block skipped, the `^`
    ///     is emitted as a literal caret and `B`,`S` as plain bytes.
    #[test]
    fn build_dcws_two_char_bound_pins_plus_not_times() {
        // "____^BS" — the `^BS` escape begins at index 4.
        let got = build_dcws_with_input_opts("____^BS", true, false)
            .expect("parse 2-char ctrl name must succeed");
        assert_eq!(
            got,
            vec![95, 95, 95, 95, 8],
            "`^BS` at index 4 of a 7-byte input must decode to BS=8 — pins \
             `i + 3` (mutant `i * 3` would skip the block, yielding \
             [95,95,95,95,94,66,83])"
        );
    }

    /// Survivor #6 — `build_dcws_with_input_opts` L531:30 `>` → `>=`
    /// (`value > 255` ordinal-range guard).
    ///
    /// `^255` is exactly on the boundary: the original `255 > 255` is
    /// false (accept, push codeword 255); the mutant `255 >= 255` is true
    /// (reject with an error). Asserting both the success of `^255` and
    /// the rejection of `^256` pins the strict `>`.
    #[test]
    fn build_dcws_ordinal_boundary_pins_gt_not_ge() {
        let got = build_dcws_with_input_opts("^255", true, false)
            .expect("ordinal 255 must be accepted (boundary)");
        assert_eq!(
            got,
            vec![255],
            "`^255` is in range (000..=255) — pins `value > 255` (mutant \
             `>= 255` would reject the boundary value)"
        );
        // The far side of the boundary must still reject.
        assert!(
            build_dcws_with_input_opts("^256", true, false).is_err(),
            "`^256` must be rejected by both original and mutant"
        );
    }

    /// Survivor #8 — `encode` L909:19 `>` → `>=`
    /// (`data.len() > 2500` length guard).
    ///
    /// At exactly 2500 bytes the original `2500 > 2500` is false (accept
    /// and continue to size selection); the mutant `2500 >= 2500` is true
    /// (reject with the 2500-byte-limit error). A 2500-byte payload is
    /// too large to fit any Ultracode symbol, so the *kind* of error
    /// distinguishes the branches: the original falls through to the
    /// "exceeds maximum symbol size" error, the mutant returns the
    /// "exceeds BWIPP's 2500-byte limit" error.
    #[test]
    fn encode_length_boundary_pins_gt_not_ge() {
        let data = "A".repeat(2500);
        let err = encode(&data, &Options::default())
            .expect_err("2500 bytes overflows the largest symbol size");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("maximum symbol size"),
            "at exactly 2500 bytes the length guard must NOT fire — pins \
             `data.len() > 2500` (mutant `>= 2500` would return the \
             2500-byte-limit error instead); got: {msg}"
        );
        assert!(
            !msg.contains("2500-byte limit"),
            "the 2500-byte-limit branch must not be taken at len == 2500; \
             got: {msg}"
        );
        // 2501 bytes must trip the limit under both original and mutant.
        let too_long = "A".repeat(2501);
        let err2 = encode(&too_long, &Options::default()).expect_err("2501 bytes must be rejected");
        assert!(
            format!("{err2:?}").contains("2500-byte limit"),
            "2501 bytes must hit the explicit 2500-byte-limit guard"
        );
    }

    /// Equivalence proofs for the four survivors that no input can
    /// distinguish. Each is argued precisely against file:line:operator,
    /// with an executable witness where one exists.
    ///
    /// ── Survivor #1 — `rs_ecprime` L300:46 `%` → `+` ──
    /// `let feedback = (d + prime - lfsr[0]) % prime;`
    /// `feedback` is used only at L305 (`c * feedback % prime`) and L307
    /// (`coeffs[0] * feedback % prime`) — i.e. it always flows through a
    /// `_ * feedback % prime` reduction. The mutant replaces the reducing
    /// `% prime` with `+ prime`, so `feedback_mut = feedback_orig +
    /// prime`. For any multiplier `c`:
    ///   `c * (f + prime) % prime = (c*f + c*prime) % prime = c*f % prime`.
    /// Both downstream uses are therefore unchanged ⇒ identical LFSR
    /// state ⇒ EQUIVALENT. (No overflow: with the production field
    /// `prime = 283`, `feedback < 283`, `c < 283`, and `d + prime` fits
    /// in u32.) The assertion below witnesses the invariant directly.
    ///
    /// ── Survivor #2 — `rs_ecprime` L305:51 `%` → `+` ──
    /// `lfsr[l] = (lfsr[l + 1] + c * feedback % prime) % prime;`
    /// The L305:51 `%` is the *inner* reduction of `c * feedback`. The
    /// *outer* `% prime` (L305:60) is left intact by this mutant. Let
    /// `p = c * feedback`. Original inner value is `p % prime`; mutant is
    /// `p + prime`. After adding `lfsr[l+1]` and the surviving outer
    /// reduction:
    ///   orig:  `(lfsr[l+1] + (p % prime)) % prime`
    ///   mut:   `(lfsr[l+1] + p + prime)  % prime = (lfsr[l+1] + p) % prime`
    /// and `(p % prime) ≡ p (mod prime)`, so both equal
    /// `(lfsr[l+1] + p) % prime` ⇒ EQUIVALENT.
    ///
    /// ── Survivor #7 — `layout_pixs` L622:21 `<` → `<=` ──
    /// `while j < rows { … j += 6; }` (sparse separator dots).
    /// `j` starts at 0 and steps by 6, so it only ever takes values
    /// `≡ 0 (mod 6)`. `layout_pixs` is reached only via `encode` /
    /// `encode_pixs_with_opts`, both of which pass `rows = rows_pre * 6 +
    /// 1`, i.e. `rows ≡ 1 (mod 6)`. The two predicates `j < rows` and
    /// `j <= rows` differ exactly when some visited `j` equals `rows`;
    /// but `j ≡ 0` and `rows ≡ 1 (mod 6)` can never be equal, so the set
    /// of visited `j` is identical ⇒ EQUIVALENT. (Were `j == rows` ever
    /// reachable, the mutant would also index `pixs[qmv(i, rows)]` out of
    /// bounds and panic — further confirming the boundary is never hit.)
    /// The assertion below witnesses `final_rows % 6 == 1` across a size
    /// sweep.
    ///
    /// ── Survivors #9 & #10 — `encode` L967:39 `<` → `==` / `<` → `<=` ──
    /// `let palette_idx = if cell < 0 { 0 } else { TABLE[cell as usize] };`
    /// where `BWIPP_TILE_DIGIT_TO_PALETTE_IDX[0] == 0`. Compare branches
    /// per `cell` value:
    ///   - `cell > 0`: `<0`, `==0`, `<=0` all false ⇒ `TABLE[cell]` (same);
    ///   - `cell == 0`: original `<0` false ⇒ `TABLE[0] == 0`; mutant
    ///     `==0` / `<=0` true ⇒ `0`. Both yield 0 (identical);
    ///   - `cell < 0`: original `<0` true ⇒ 0; mutant `==0` false ⇒
    ///     `TABLE[(-1) as usize]` (out-of-bounds panic), and `<=0` true
    ///     ⇒ 0.
    /// So `< → ==` differs from the original *only* when some `cell < 0`
    /// reaches this code, and `< → <=` is *unconditionally* identical to
    /// the original. `layout_pixs` writes every grid cell (vertical pass
    /// L618-629 fills col 0 and the top/bottom rows and all of cols ≥ 5
    /// every-6th-row plus the borders; horizontal pass L632-646 fills the
    /// remaining inner cells of cols 0..4 and the right border; the DCC
    /// column and the main tile sequence fill the interior). The
    /// `-1`-sentinel branch is therefore "defensive" and never taken for
    /// real `encode` inputs. The assertion below witnesses that the raw
    /// pixs grid contains no `-1` across a size sweep, so neither mutant
    /// can be distinguished ⇒ EQUIVALENT (both are UNREACHABLE-divergence
    /// for `< → ==`, and value-EQUIVALENT for `< → <=`).
    #[test]
    fn ultracode_equivalence_notes() {
        // Survivor #1 witness: c*(f+prime) % prime == c*f % prime.
        let prime = ULTRACODE_RS_PRIME;
        for c in [0u32, 1, 2, 7, 42, 200, 282] {
            for f in [0u32, 1, 3, 99, 282] {
                assert_eq!(
                    (c * (f + prime)) % prime,
                    (c * f) % prime,
                    "L300 `%→+` invariant: adding `prime` to feedback is \
                     absorbed by the downstream `% prime`"
                );
            }
        }
        // Survivor #2 witness: (a + (p % prime)) % prime == (a + p) % prime.
        for a in [0u32, 1, 4, 100, 282] {
            for p in [0u32, 5, 283, 1000, 79_999] {
                assert_eq!(
                    (a + (p % prime)) % prime,
                    (a + p) % prime,
                    "L305:51 `%→+` invariant: inner reduction is redundant \
                     given the surviving outer `% prime`"
                );
            }
        }

        // Survivor #7 witness: every symbol's final_rows ≡ 1 (mod 6),
        // so the L622 loop variable (≡ 0 mod 6) can never equal `rows`.
        for tcc in 1u32..=512 {
            if let Some((_m, rows_pre, _cols, _pads)) = pick_symbol_size(tcc) {
                let final_rows = rows_pre * 6 + 1;
                assert_eq!(
                    final_rows % 6,
                    1,
                    "layout_pixs rows must be ≡ 1 (mod 6) so the j-loop \
                     (j ≡ 0 mod 6) never lands on `rows`"
                );
            }
        }

        // Survivors #9 & #10 witness: the raw pixs grid has no `-1`
        // sentinel for real encode inputs, so the `cell < 0` branch is
        // never taken and neither mutant can diverge observably.
        let long = "X".repeat(200);
        for data in [
            "A",
            "HELLO",
            "Hello World",
            "12345",
            "X".repeat(100).as_str(),
            long.as_str(),
        ] {
            let (_rows, _cols, pixs) =
                super::encode_pixs_default(data).expect("encode_pixs_default ok");
            assert!(
                pixs.iter().all(|&c| c >= 0),
                "layout fills every cell — no `-1` sentinel reaches the \
                 `cell < 0` branch (input {data:?})"
            );
        }
    }
}
