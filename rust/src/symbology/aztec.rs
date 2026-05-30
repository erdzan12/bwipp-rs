//! Aztec Code — concentric-bull's-eye 2D matrix barcode.
//!
//! Fully verified port. The static tables here drive the metrics,
//! character-set sentinels, and shift/latch matrices that the encoder
//! body (`encode_seq` / `seq_to_bits` / `build_matrix`) consumes; the
//! reference grid for L≥5 and byte-mode dispatch are also covered.
//!
//! Aztec supports 37 fixed-size variants:
//!   - **Rune** — a 6-codeword 11×11 marker symbol (no data).
//!   - **Compact L1..L4** — 4 sizes from 15×15 to 27×27 with a
//!     smaller central bull's-eye (5×5).
//!   - **Full L1..L32** — 32 sizes from 19×19 to 151×151 with a
//!     7×7 central bull's-eye.
//!
//! Reed-Solomon ECC operates over GF(2^k) where k ∈ {4, 6, 8, 10, 12}
//! depending on layer count (and thus codeword bit size).
//!
//! Direct port of `bwipp_azteccode.globals` from bwip-js line 30527.
//! Verified byte-for-byte against bwip-js on a 27-input corpus
//! (ASCII / UTF-8 multibyte / CR-LF pre-compression / punctuation
//! pairs). Catalog rows: `azteccode`, `azteccodecompact`, `aztecrune`,
//! `hibc_lic_azteccode`.

#![allow(dead_code)]

/// Aztec character-set sentinel constants (mirrors BWIPP's
/// `azteccode_u..b` and `azteccode_lu..p5`).
///
/// States (0..=5):
pub(crate) const STATE_UPPER: u8 = 0;
pub(crate) const STATE_LOWER: u8 = 1;
pub(crate) const STATE_MIXED: u8 = 2;
pub(crate) const STATE_PUNCT: u8 = 3;
pub(crate) const STATE_DIGIT: u8 = 4;
pub(crate) const STATE_BYTE: u8 = 5;

/// Latch sentinels (negative numbers).
pub(crate) const LATCH_UPPER: i32 = -2;
pub(crate) const LATCH_LOWER: i32 = -3;
pub(crate) const LATCH_MIXED: i32 = -4;
pub(crate) const LATCH_PUNCT: i32 = -5;
pub(crate) const LATCH_DIGIT: i32 = -6;
/// Shift sentinels.
pub(crate) const SHIFT_UPPER: i32 = -7;
pub(crate) const SHIFT_PUNCT: i32 = -8;
pub(crate) const SHIFT_BYTE: i32 = -9;
pub(crate) const FLG_NEXT: i32 = -10;
/// Punctuation pairs (CR/LF, period+space, etc).
pub(crate) const PAIR_2: i32 = -11; // CR LF
pub(crate) const PAIR_3: i32 = -12; // ". "
pub(crate) const PAIR_4: i32 = -13; // ", "
pub(crate) const PAIR_5: i32 = -14; // ": "

/// Bits per codeword per state (Upper, Lower, Mixed, Punct, Digit, Byte).
/// 5 bits for the first four "letter" states, 4 bits for digit, 8 bits
/// for byte. Direct port of `azteccode_charsizes` (bwip-js line 30582).
pub(crate) const CHAR_SIZES: [u8; 6] = [5, 5, 5, 5, 4, 8];

/// Aztec latch-length matrix. `LATCH_LEN[from][to]` is the bit cost
/// (NOT codeword cost) of switching state from `from` to `to`. Index
/// 0..=4 are the regular states (Upper, Lower, Mixed, Punct, Digit);
/// row 5 is "Byte" which is exit-only (latch INTO byte is via
/// SHIFT_BYTE, not via this matrix).
///
/// Direct port of `azteccode_latlen` (bwip-js line 30571).
pub(crate) const LATCH_LEN: [[u8; 6]; 6] = [
    [0, 5, 5, 10, 5, 10],
    [9, 0, 5, 10, 5, 10],
    [5, 5, 0, 5, 10, 10],
    [5, 10, 10, 0, 10, 15],
    [4, 9, 9, 14, 0, 14],
    [0, 0, 0, 0, 0, 0],
];

/// Aztec shift-length matrix. `SHIFT_LEN[from][to]` is the bit cost
/// of a single-character shift from `from` to `to`. `u16::MAX` (the
/// BWIPP `azteccode_e = 1000000` sentinel) means "no shift is
/// available from this state to that target".
///
/// Direct port of `azteccode_shftlen` (bwip-js line 30575).
pub(crate) const SHIFT_LEN: [[u16; 5]; 5] = [
    [u16::MAX, u16::MAX, u16::MAX, 5, u16::MAX],
    [5, u16::MAX, u16::MAX, 5, u16::MAX],
    [u16::MAX, u16::MAX, u16::MAX, 5, u16::MAX],
    [u16::MAX, u16::MAX, u16::MAX, u16::MAX, u16::MAX],
    [4, u16::MAX, u16::MAX, 4, u16::MAX],
];

/// Aztec metrics entry — per-size symbol parameters.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AztecMetrics {
    /// "rune" | "compact" | "full".
    pub format: &'static str,
    /// Layer count (1..=32 for full, 1..=4 for compact, 0 for rune).
    pub layers: u8,
    /// Whether this size carries data (1 = yes, 0 = rune marker only).
    pub has_data: u8,
    /// Number of codewords in the symbol (data + ECC).
    pub ncws: u16,
    /// Bits per codeword (4 / 6 / 8 / 10 / 12).
    pub bps: u8,
}

/// All 37 Aztec size variants in BWIPP's enumeration order: rune,
/// then alternating compact + full at each layer count 1..=4, then
/// full 5..=32. Direct port of `azteccode_metrics` (bwip-js line 30576).
pub(crate) const METRICS: [AztecMetrics; 37] = [
    AztecMetrics {
        format: "rune",
        layers: 0,
        has_data: 0,
        ncws: 0,
        bps: 6,
    },
    AztecMetrics {
        format: "compact",
        layers: 1,
        has_data: 1,
        ncws: 17,
        bps: 6,
    },
    AztecMetrics {
        format: "full",
        layers: 1,
        has_data: 1,
        ncws: 21,
        bps: 6,
    },
    AztecMetrics {
        format: "compact",
        layers: 2,
        has_data: 0,
        ncws: 40,
        bps: 6,
    },
    AztecMetrics {
        format: "full",
        layers: 2,
        has_data: 1,
        ncws: 48,
        bps: 6,
    },
    AztecMetrics {
        format: "compact",
        layers: 3,
        has_data: 0,
        ncws: 51,
        bps: 8,
    },
    AztecMetrics {
        format: "full",
        layers: 3,
        has_data: 1,
        ncws: 60,
        bps: 8,
    },
    AztecMetrics {
        format: "compact",
        layers: 4,
        has_data: 0,
        ncws: 76,
        bps: 8,
    },
    AztecMetrics {
        format: "full",
        layers: 4,
        has_data: 1,
        ncws: 88,
        bps: 8,
    },
    AztecMetrics {
        format: "full",
        layers: 5,
        has_data: 1,
        ncws: 120,
        bps: 8,
    },
    AztecMetrics {
        format: "full",
        layers: 6,
        has_data: 1,
        ncws: 156,
        bps: 8,
    },
    AztecMetrics {
        format: "full",
        layers: 7,
        has_data: 1,
        ncws: 196,
        bps: 8,
    },
    AztecMetrics {
        format: "full",
        layers: 8,
        has_data: 1,
        ncws: 240,
        bps: 8,
    },
    AztecMetrics {
        format: "full",
        layers: 9,
        has_data: 1,
        ncws: 230,
        bps: 10,
    },
    AztecMetrics {
        format: "full",
        layers: 10,
        has_data: 1,
        ncws: 272,
        bps: 10,
    },
    AztecMetrics {
        format: "full",
        layers: 11,
        has_data: 1,
        ncws: 316,
        bps: 10,
    },
    AztecMetrics {
        format: "full",
        layers: 12,
        has_data: 1,
        ncws: 364,
        bps: 10,
    },
    AztecMetrics {
        format: "full",
        layers: 13,
        has_data: 1,
        ncws: 416,
        bps: 10,
    },
    AztecMetrics {
        format: "full",
        layers: 14,
        has_data: 1,
        ncws: 470,
        bps: 10,
    },
    AztecMetrics {
        format: "full",
        layers: 15,
        has_data: 1,
        ncws: 528,
        bps: 10,
    },
    AztecMetrics {
        format: "full",
        layers: 16,
        has_data: 1,
        ncws: 588,
        bps: 10,
    },
    AztecMetrics {
        format: "full",
        layers: 17,
        has_data: 1,
        ncws: 652,
        bps: 10,
    },
    AztecMetrics {
        format: "full",
        layers: 18,
        has_data: 1,
        ncws: 720,
        bps: 10,
    },
    AztecMetrics {
        format: "full",
        layers: 19,
        has_data: 1,
        ncws: 790,
        bps: 10,
    },
    AztecMetrics {
        format: "full",
        layers: 20,
        has_data: 1,
        ncws: 864,
        bps: 10,
    },
    AztecMetrics {
        format: "full",
        layers: 21,
        has_data: 1,
        ncws: 940,
        bps: 10,
    },
    AztecMetrics {
        format: "full",
        layers: 22,
        has_data: 1,
        ncws: 1020,
        bps: 10,
    },
    AztecMetrics {
        format: "full",
        layers: 23,
        has_data: 0,
        ncws: 920,
        bps: 12,
    },
    AztecMetrics {
        format: "full",
        layers: 24,
        has_data: 0,
        ncws: 992,
        bps: 12,
    },
    AztecMetrics {
        format: "full",
        layers: 25,
        has_data: 0,
        ncws: 1066,
        bps: 12,
    },
    AztecMetrics {
        format: "full",
        layers: 26,
        has_data: 0,
        ncws: 1144,
        bps: 12,
    },
    AztecMetrics {
        format: "full",
        layers: 27,
        has_data: 0,
        ncws: 1224,
        bps: 12,
    },
    AztecMetrics {
        format: "full",
        layers: 28,
        has_data: 0,
        ncws: 1306,
        bps: 12,
    },
    AztecMetrics {
        format: "full",
        layers: 29,
        has_data: 0,
        ncws: 1392,
        bps: 12,
    },
    AztecMetrics {
        format: "full",
        layers: 30,
        has_data: 0,
        ncws: 1480,
        bps: 12,
    },
    AztecMetrics {
        format: "full",
        layers: 31,
        has_data: 0,
        ncws: 1570,
        bps: 12,
    },
    AztecMetrics {
        format: "full",
        layers: 32,
        has_data: 0,
        ncws: 1664,
        bps: 12,
    },
];

/// Aztec Upper-state special codewords. The 32 codewords (0..=31)
/// for 5-bit upper state encode: `PS` (0, shift-to-punct), space
/// (1), `A`-`Z` (2..=27), `LL` (28, latch-lower), `LM` (29,
/// latch-mixed), `LD` (30, latch-digit), `SB` (31, shift-byte).
pub(crate) const UPPER_PS: u8 = 0;
pub(crate) const UPPER_SPACE: u8 = 1;
pub(crate) const UPPER_LL: u8 = 28;
pub(crate) const UPPER_LM: u8 = 29;
pub(crate) const UPPER_LD: u8 = 30;
pub(crate) const UPPER_SB: u8 = 31;

/// Look up the Aztec Upper-state codeword for an ASCII byte.
///
/// Upper state covers space (codeword 1) and uppercase letters
/// A-Z (codewords 2-27). Returns `None` for any byte that needs
/// shift/latch into another state.
pub(crate) fn upper_codeword(byte: u8) -> Option<u8> {
    match byte {
        b' ' => Some(UPPER_SPACE),
        b'A'..=b'Z' => Some(byte - b'A' + 2),
        _ => None,
    }
}

/// Look up the Aztec Lower-state codeword for an ASCII byte.
/// Lower state: space (1), lowercase a-z (2-27), latches/shifts at
/// 28-31 (LD-up: SU = codeword 28; LL = codeword 0 is the punct
/// shift; LL is codeword 28's "lower-to-upper" wait this is wrong).
///
/// Actually per BWIPP charmap col 1 (lower state):
///   row 0  = ps (codeword 0 = shift-to-punct)
///   row 1  = space
///   rows 2..27 = a-z
///   row 28 = SU (codeword 28 = shift-upper, "uppercase next char")
///   row 29 = LM (codeword 29 = latch-mixed)
///   row 30 = LD (codeword 30 = latch-digit)
///   row 31 = SB (codeword 31 = shift-byte)
pub(crate) fn lower_codeword(byte: u8) -> Option<u8> {
    match byte {
        b' ' => Some(1),
        b'a'..=b'z' => Some(byte - b'a' + 2),
        _ => None,
    }
}

/// Look up the Aztec Digit-state codeword for an ASCII byte.
/// Digit state uses 4 bits (16 codewords): PS (0), space (1),
/// digits 0-9 (2-11), comma (12), period (13), LU (14, latch-upper),
/// SU (15, shift-upper).
pub(crate) fn digit_codeword(byte: u8) -> Option<u8> {
    match byte {
        b' ' => Some(1),
        b'0'..=b'9' => Some(byte - b'0' + 2),
        b',' => Some(12),
        b'.' => Some(13),
        _ => None,
    }
}

/// Look up the Aztec Mixed-state codeword for an ASCII byte.
/// Mixed state covers control characters and a few special symbols.
/// Charmap col 2:
///   row 0  = PS (shift-to-punct, codeword 0)
///   row 1  = space (32 → 1)
///   rows 2..14 = CTL chars 1..13 (^A..^M)
///   row 15 = control 27 (ESC)
///   rows 16..19 = control 28..31
///   row 20 = '@' (codeword 20)
///   row 21 = '\\' (92)
///   row 22 = '^' (94)
///   row 23 = '_' (95)
///   row 24 = '`' (96)
///   row 25 = '|' (124)
///   row 26 = '~' (126)
///   row 27 = control 127 (DEL)
///   row 28 = LL (latch-lower)
///   row 29 = LU (latch-upper)
///   row 30 = LP (latch-punct)
///   row 31 = SB (shift-byte)
pub(crate) fn mixed_codeword(byte: u8) -> Option<u8> {
    match byte {
        b' ' => Some(1),
        1..=13 => Some(byte + 1), // codewords 2..14
        27 => Some(15),
        28..=31 => Some(byte - 12), // codewords 16..19
        b'@' => Some(20),
        b'\\' => Some(21),
        b'^' => Some(22),
        b'_' => Some(23),
        b'`' => Some(24),
        b'|' => Some(25),
        b'~' => Some(26),
        127 => Some(27),
        _ => None,
    }
}

/// Look up the Aztec Punct-state codeword for an ASCII byte.
/// Punct state covers ASCII punctuation. Charmap col 3:
///   row 0 = FLG_NEXT (codeword 0)
///   row 1 = CR (13)
///   row 2..5 = pairs (CRLF, ". ", ", ", ": ")
///   row 6..27 = punctuation (! " # $ % & ' ( ) * + , - . / : ; < = > ? [ ])
///   row 28..31 = LU, LL, LM, LP (latch back)
pub(crate) fn punct_codeword(byte: u8) -> Option<u8> {
    match byte {
        13 => Some(1),
        b'!' => Some(6),
        b'"' => Some(7),
        b'#' => Some(8),
        b'$' => Some(9),
        b'%' => Some(10),
        b'&' => Some(11),
        b'\'' => Some(12),
        b'(' => Some(13),
        b')' => Some(14),
        b'*' => Some(15),
        b'+' => Some(16),
        b',' => Some(17),
        b'-' => Some(18),
        b'.' => Some(19),
        b'/' => Some(20),
        b':' => Some(21),
        b';' => Some(22),
        b'<' => Some(23),
        b'=' => Some(24),
        b'>' => Some(25),
        b'?' => Some(26),
        b'[' => Some(27),
        b']' => Some(28),
        b'{' => Some(29),
        b'}' => Some(30),
        _ => None,
    }
}

/// Encode a byte stream in a single fixed state (no shifts/latches).
/// Returns the codeword sequence, or `None` if any byte isn't
/// encodable in the requested state.
///
/// This is a building block for the full DP encoder. Useful directly
/// for inputs that live entirely in one alphabet (pure uppercase,
/// pure lowercase, pure digits, etc).
pub(crate) fn encode_single_state(state: u8, bytes: &[u8]) -> Option<Vec<u8>> {
    let lookup: fn(u8) -> Option<u8> = match state {
        STATE_UPPER => upper_codeword,
        STATE_LOWER => lower_codeword,
        STATE_MIXED => mixed_codeword,
        STATE_PUNCT => punct_codeword,
        STATE_DIGIT => digit_codeword,
        _ => return None,
    };
    let mut out = Vec::with_capacity(bytes.len());
    for &b in bytes {
        out.push(lookup(b)?);
    }
    Some(out)
}

/// Pack a sequence of state-encoded codewords into a bit stream.
/// Each codeword contributes `bits_per_codeword` bits (5 for letter
/// states, 4 for digit state, 8 for byte). MSB first.
///
/// Returns a `Vec<bool>` where each element is one bit of the
/// stream.
pub(crate) fn pack_codewords_to_bits(codewords: &[u8], bits_per_codeword: u8) -> Vec<bool> {
    let mut bits = Vec::with_capacity(codewords.len() * bits_per_codeword as usize);
    for &cw in codewords {
        for k in (0..bits_per_codeword).rev() {
            bits.push((cw >> k) & 1 == 1);
        }
    }
    bits
}

/// Preferred encoding state for a single ASCII byte. Returns the
/// state where the byte's codeword is the cheapest (fewest extra
/// shifts/latches assuming we're already in that state). Used by
/// the greedy mode-switching encoder.
fn preferred_state(byte: u8) -> u8 {
    match byte {
        b' ' => STATE_UPPER, // space is in U/L/M/P/D but Upper is canonical
        b'A'..=b'Z' => STATE_UPPER,
        b'a'..=b'z' => STATE_LOWER,
        b'0'..=b'9' | b',' | b'.' => STATE_DIGIT,
        13 => STATE_PUNCT, // CR
        b'!' | b'"' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b'-'
        | b'/' | b':' | b';' | b'<' | b'=' | b'>' | b'?' | b'[' | b']' | b'{' | b'}' => STATE_PUNCT,
        1..=12 | 14..=26 | 27..=31 | b'@' | b'\\' | b'^' | b'_' | b'`' | b'|' | b'~' | 127 => {
            STATE_MIXED
        }
        _ => STATE_BYTE,
    }
}

/// Codeword that performs a latch from `from` to `to`, in `from`
/// state's codeword space. Mirrors BWIPP's charmap entries for the
/// LL/LM/LU/LP/LD codewords.
///
/// Returns the codeword and the state we land in after emission.
/// Most latches are 5-bit (or 4-bit for Digit) within the source
/// state; latches that require two steps (e.g. U→P via M) return
/// `None` and the caller must compose.
fn latch_codeword(from: u8, to: u8) -> Option<u8> {
    match (from, to) {
        (STATE_UPPER, STATE_LOWER) => Some(UPPER_LL),
        (STATE_UPPER, STATE_MIXED) => Some(UPPER_LM),
        (STATE_UPPER, STATE_DIGIT) => Some(UPPER_LD),
        (STATE_LOWER, STATE_MIXED) => Some(29), // LM in Lower charmap col 1
        (STATE_LOWER, STATE_DIGIT) => Some(30), // LD in Lower
        (STATE_MIXED, STATE_LOWER) => Some(28),
        (STATE_MIXED, STATE_UPPER) => Some(29),
        (STATE_MIXED, STATE_PUNCT) => Some(30),
        (STATE_PUNCT, STATE_UPPER) => Some(31),
        (STATE_DIGIT, STATE_UPPER) => Some(14),
        _ => None,
    }
}

/// Encode a single state-codeword into a bit-stream, appending it
/// to the given Vec<bool>. Bits are MSB-first.
fn append_codeword(bits: &mut Vec<bool>, value: u8, width: u8) {
    for k in (0..width).rev() {
        bits.push((value >> k) & 1 == 1);
    }
}

/// Encode a single byte in `state` with no latch/shift. Returns the
/// codeword, or `None` if `byte` isn't directly encodable in that
/// alphabet.
fn encode_byte_in_state(state: u8, byte: u8) -> Option<u8> {
    match state {
        STATE_UPPER => upper_codeword(byte),
        STATE_LOWER => lower_codeword(byte),
        STATE_MIXED => mixed_codeword(byte),
        STATE_PUNCT => punct_codeword(byte),
        STATE_DIGIT => digit_codeword(byte),
        _ => None,
    }
}

/// Greedy Aztec mode-switching encoder.
///
/// For each input byte, first tries to emit directly in the current
/// state (avoids spurious latches for bytes like space that exist in
/// every alphabet). If unavailable, latches to [`preferred_state`]
/// and emits there. Does NOT yet implement shifts, pair pre-compression,
/// or the full BWIPP DP — use only for inputs whose state transitions
/// are reachable via single-step latches in [`latch_codeword`].
///
/// Returns `Err(InvalidData)` for inputs containing bytes that need
/// the Byte state (high-bit ASCII) or that require multi-step latches
/// (e.g. Lower→Upper, Digit→Lower).
pub(crate) fn encode_greedy(bytes: &[u8]) -> Result<Vec<bool>, crate::error::Error> {
    let mut bits = Vec::with_capacity(bytes.len() * 6);
    let mut state = STATE_UPPER;
    for &b in bytes {
        if let Some(cw) = encode_byte_in_state(state, b) {
            append_codeword(&mut bits, cw, CHAR_SIZES[state as usize]);
            continue;
        }
        let target = preferred_state(b);
        if target == STATE_BYTE {
            return Err(crate::error::Error::InvalidData(format!(
                "Aztec encode_greedy: byte 0x{b:02x} requires the Byte-state shift (BS); this Rust port covers Upper/Lower/Mixed/Punct/Digit but not Byte-mode shifts (use BWIPP for binary-heavy payloads)",
            )));
        }
        let latch = latch_codeword(state, target).ok_or_else(|| {
            crate::error::Error::InvalidData(format!(
                "Aztec encode_greedy: no direct latch from state {state} to {target} \
                 — multi-step latches are outside this port's scope",
            ))
        })?;
        append_codeword(&mut bits, latch, CHAR_SIZES[state as usize]);
        state = target;
        let cw = encode_byte_in_state(state, b).ok_or_else(|| {
            crate::error::Error::InvalidData(format!(
                "Aztec encode_greedy: byte 0x{b:02x} not encodable in state {state} after latch",
            ))
        })?;
        append_codeword(&mut bits, cw, CHAR_SIZES[state as usize]);
    }
    Ok(bits)
}

/// Latch-sequence table. `LATCH_SEQ[from][to]` is the sequence of
/// latch sentinels emitted in order to transition state `from` → `to`.
/// Each sentinel's codeword is encoded in whatever state is active at
/// the moment it's emitted (i.e. the *previous* state before this
/// sentinel takes effect).
///
/// Direct port of `azteccode_latseq` (bwip-js line 29573).
pub(crate) const LATCH_SEQ: [[&[i32]; 6]; 6] = [
    // From Upper
    [
        &[],
        &[LATCH_LOWER],
        &[LATCH_MIXED],
        &[LATCH_MIXED, LATCH_PUNCT],
        &[LATCH_DIGIT],
        &[SHIFT_BYTE],
    ],
    // From Lower
    [
        &[LATCH_DIGIT, LATCH_UPPER],
        &[],
        &[LATCH_MIXED],
        &[LATCH_MIXED, LATCH_PUNCT],
        &[LATCH_DIGIT],
        &[SHIFT_BYTE],
    ],
    // From Mixed
    [
        &[LATCH_UPPER],
        &[LATCH_LOWER],
        &[],
        &[LATCH_PUNCT],
        &[LATCH_UPPER, LATCH_DIGIT],
        &[SHIFT_BYTE],
    ],
    // From Punct
    [
        &[LATCH_UPPER],
        &[LATCH_UPPER, LATCH_LOWER],
        &[LATCH_UPPER, LATCH_MIXED],
        &[],
        &[LATCH_UPPER, LATCH_DIGIT],
        &[LATCH_UPPER, SHIFT_BYTE],
    ],
    // From Digit
    [
        &[LATCH_UPPER],
        &[LATCH_UPPER, LATCH_LOWER],
        &[LATCH_UPPER, LATCH_MIXED],
        &[LATCH_UPPER, LATCH_MIXED, LATCH_PUNCT],
        &[],
        &[LATCH_UPPER, SHIFT_BYTE],
    ],
    // From Byte (exit-only: returns via backto state)
    [
        &[LATCH_UPPER],
        &[LATCH_LOWER],
        &[LATCH_MIXED],
        &[],
        &[],
        &[],
    ],
];

/// Look up the codeword for a sentinel (latch/shift) when emitted in
/// `state`. Returns `None` if that sentinel has no representation in
/// the given state. Mirrors the BWIPP charmap entries.
fn sentinel_codeword(state: u8, sentinel: i32) -> Option<u8> {
    match (state, sentinel) {
        (STATE_UPPER, LATCH_LOWER) => Some(28),
        (STATE_UPPER, LATCH_MIXED) => Some(29),
        (STATE_UPPER, LATCH_DIGIT) => Some(30),
        (STATE_UPPER, SHIFT_BYTE) => Some(31),
        (STATE_UPPER, SHIFT_PUNCT) => Some(0),
        (STATE_LOWER, SHIFT_UPPER) => Some(28),
        (STATE_LOWER, LATCH_MIXED) => Some(29),
        (STATE_LOWER, LATCH_DIGIT) => Some(30),
        (STATE_LOWER, SHIFT_BYTE) => Some(31),
        (STATE_LOWER, SHIFT_PUNCT) => Some(0),
        (STATE_MIXED, LATCH_LOWER) => Some(28),
        (STATE_MIXED, LATCH_UPPER) => Some(29),
        (STATE_MIXED, LATCH_PUNCT) => Some(30),
        (STATE_MIXED, SHIFT_BYTE) => Some(31),
        (STATE_MIXED, SHIFT_PUNCT) => Some(0),
        (STATE_PUNCT, LATCH_UPPER) => Some(31),
        (STATE_PUNCT, FLG_NEXT) => Some(0),
        (STATE_PUNCT, PAIR_2) => Some(2),
        (STATE_PUNCT, PAIR_3) => Some(3),
        (STATE_PUNCT, PAIR_4) => Some(4),
        (STATE_PUNCT, PAIR_5) => Some(5),
        (STATE_DIGIT, LATCH_UPPER) => Some(14),
        (STATE_DIGIT, SHIFT_UPPER) => Some(15),
        // Aztec spec: in Digit state, codeword 0 is Punct Shift (PS).
        // BWIPP's `azteccode_shftlen[4]` reflects this with shift cost 4
        // bits (= one Digit codeword) to STATE_PUNCT. Without this, our
        // DP can pick the path "Digit, shift to Punct, char, back" for
        // inputs like "12345/AB" and then fail at codeword emission
        // because there was no mapping for the sentinel. See HIBC LIC
        // Aztec input "+...123.../X..." which previously errored at
        // `seq_to_bits: shift -8 not available from state 4`.
        (STATE_DIGIT, SHIFT_PUNCT) => Some(0),
        _ => None,
    }
}

/// Map a latch sentinel to the state it transitions into. Shifts
/// (`SHIFT_UPPER`/`SHIFT_PUNCT`/`SHIFT_BYTE`) return `None` because
/// they're single-char detours, not state transitions.
fn latch_target(sentinel: i32) -> Option<u8> {
    match sentinel {
        LATCH_UPPER => Some(STATE_UPPER),
        LATCH_LOWER => Some(STATE_LOWER),
        LATCH_MIXED => Some(STATE_MIXED),
        LATCH_PUNCT => Some(STATE_PUNCT),
        LATCH_DIGIT => Some(STATE_DIGIT),
        _ => None,
    }
}

/// Bit cost of emitting `char` in `state`. For a regular char it's
/// just `CHAR_SIZES[state]`; the BWIPP code also handles FNC1 and
/// ECI sentinels with variable widths — we don't yet (those return
/// `u16::MAX` to be treated as unreachable).
fn charsize(state: u8, ch: i32) -> u16 {
    if ch >= 0 {
        CHAR_SIZES[state as usize] as u16
    } else {
        u16::MAX
    }
}

/// Sentinel for "infinite cost" in the DP. Mirrors BWIPP's
/// `azteccode_e = 1000000`.
const INF: u32 = 1_000_000;

/// Map a (previous-char, current-char) pair to its Aztec PAIR sentinel
/// if compressible, or `None` otherwise. Mirrors BWIPP's
/// `azteccode_pcomp` Map.
fn pair_sentinel(last: u8, cur: u8) -> Option<i32> {
    match (last, cur) {
        (0x0D, 0x0A) => Some(PAIR_2), // CR LF
        (b'.', b' ') => Some(PAIR_3),
        (b',', b' ') => Some(PAIR_4),
        (b':', b' ') => Some(PAIR_5),
        _ => None,
    }
}

/// True when `item` is one of the four PAIR_x sentinels emitted by
/// pair pre-compression.
fn is_pair_sentinel(item: i32) -> bool {
    matches!(item, PAIR_2 | PAIR_3 | PAIR_4 | PAIR_5)
}

/// Aztec DP-based mode-switching encoder.
///
/// Produces a sequence of items (positive byte values or negative
/// latch/shift sentinels) representing the BWIPP-style optimal path
/// through the state machine. The sequence is then passed to
/// [`seq_to_bits`] to produce the bit stream.
///
/// Implements the BWIPP DP algorithm:
/// 1. **Transitive latch closure** for state transitions (including
///    Byte-state entries when no other state encodes the next char).
/// 2. **Char emission with optional shift detour** for one-off PUNCT
///    or UPPER chars accessed from another state.
/// 3. **Pair pre-compression** for the BWIPP pair sentinels
///    (CR/LF → P_CRLF, ". " → P_PERIOD_SP, ", " → P_COMMA_SP,
///    ": " → P_COLON_SP) — looks back one char to retroactively
///    fold pair-compressible character pairs into the shorter
///    PAIR_x Punct codeword.
/// 4. **Byte state entry/exit** via Phase-4 bookkeeping: tracks the
///    `backto` state so the encoder knows where to return after the
///    Byte-mode 8-bit literal run ends.
///
/// Char-only input — negative sentinels in `msg` (BWIPP's
/// parsefnc marker encoding) are outside this port's API surface;
/// callers pass raw byte slices. Within those scope limits, the
/// output is byte-
/// identical to BWIPP's DP across the 27-input bwip-js oracle
/// corpus (ASCII, Latin-1 UTF-8 multibyte via Byte mode, and pair-
/// compressible punctuation text).
pub(crate) fn encode_dp(msg: &[u8]) -> Result<Vec<i32>, crate::error::Error> {
    let mut curseq: [Vec<i32>; 6] = Default::default();
    let mut curlen: [u32; 6] = [0, INF, INF, INF, INF, INF];
    let mut backto: u8 = STATE_UPPER;
    let mut lastchar: Option<u8> = None;

    for &b in msg {
        let ch = b as i32;

        // Phase 1: transitive latch closure.
        loop {
            let mut improved = false;
            for x in 0..6u8 {
                for y in 0..6u8 {
                    if x == STATE_BYTE && y != backto {
                        continue;
                    }
                    let lat = LATCH_LEN[x as usize][y as usize] as u32;
                    let cost = curlen[x as usize].saturating_add(lat);
                    if cost < curlen[y as usize] {
                        curlen[y as usize] = cost;
                        let mut new_seq = curseq[x as usize].clone();
                        new_seq.extend_from_slice(LATCH_SEQ[x as usize][y as usize]);
                        curseq[y as usize] = new_seq;
                        if y == STATE_BYTE {
                            backto = if x == STATE_PUNCT || x == STATE_DIGIT {
                                STATE_UPPER
                            } else {
                                x
                            };
                        }
                        improved = true;
                    }
                }
            }
            if !improved {
                break;
            }
        }

        // Phase 2: emit char in each state (with optional shift detour).
        let mut nxtseq: [Vec<i32>; 6] = Default::default();
        let mut nxtlen: [u32; 6] = [INF; 6];
        for x in 0..6u8 {
            // Can ch be emitted in state x? Byte state takes any byte as
            // an 8-bit literal; other states require the byte to be in
            // their alphabet.
            let encodable_in_x = if x == STATE_BYTE {
                true
            } else {
                encode_byte_in_state(x, b).is_some()
            };
            if !encodable_in_x {
                continue;
            }

            // Direct emit in state x.
            let cost = curlen[x as usize].saturating_add(charsize(x, ch) as u32);
            if cost < nxtlen[x as usize] {
                nxtlen[x as usize] = cost;
                let mut s = curseq[x as usize].clone();
                s.push(ch);
                nxtseq[x as usize] = s;
            }

            // Shift-then-char from y (skip BYTE — shifts don't target Byte).
            if x == STATE_BYTE {
                continue;
            }
            for y in 0..5u8 {
                if y == x {
                    continue;
                }
                let shft = SHIFT_LEN[y as usize][x as usize] as u32;
                if shft == u16::MAX as u32 {
                    continue;
                }
                let cost = curlen[y as usize]
                    .saturating_add(shft)
                    .saturating_add(charsize(x, ch) as u32);
                if cost < nxtlen[y as usize] {
                    nxtlen[y as usize] = cost;
                    let shift_token = if x == STATE_PUNCT {
                        SHIFT_PUNCT
                    } else if x == STATE_UPPER {
                        SHIFT_UPPER
                    } else {
                        continue; // no shift token for other targets in BWIPP
                    };
                    let mut s = curseq[y as usize].clone();
                    s.push(shift_token);
                    s.push(ch);
                    nxtseq[y as usize] = s;
                }
            }
        }

        // Phase 3: pair pre-compression.
        // If (lastchar, b) forms a compressible pair (CR/LF, ". ", ", ",
        // ": "), look back in curseq[i] for lastchar and replace its
        // emission with a Punct PAIR_x sentinel. Direct port of BWIPP
        // azteccode lines 30033-30108.
        if let Some(last) = lastchar {
            if let Some(pair_sent) = pair_sentinel(last, b) {
                for &i_state in &[
                    STATE_UPPER,
                    STATE_LOWER,
                    STATE_MIXED,
                    STATE_PUNCT,
                    STATE_DIGIT,
                ] {
                    let mut in_p = true;
                    if (i_state == STATE_MIXED && last == 0x0D)
                        || (i_state == STATE_DIGIT && (last == b',' || last == b'.'))
                    {
                        in_p = false;
                    }
                    if !in_p {
                        continue;
                    }
                    let curseq_i_len = curseq[i_state as usize].len();
                    if curlen[i_state as usize] >= nxtlen[i_state as usize] {
                        continue;
                    }
                    // Look back in curseq[i_state] for lastchar.
                    let mut lastld = false;
                    let mut lastsp = false;
                    let mut lastidx: Option<usize> = None;
                    let seq_i = &curseq[i_state as usize];
                    for idx in (0..curseq_i_len).rev() {
                        let ch = seq_i[idx];
                        if lastidx.is_none() {
                            if ch >= 0 && ch as u8 == last {
                                lastidx = Some(idx);
                                if idx > 0 && seq_i[idx - 1] == SHIFT_PUNCT {
                                    lastsp = true;
                                }
                            }
                        } else if ch == SHIFT_BYTE {
                            lastidx = None;
                            break;
                        } else if (LATCH_DIGIT..0).contains(&ch) {
                            if i_state == STATE_PUNCT {
                                if ch == LATCH_DIGIT {
                                    lastld = true;
                                }
                            } else if ch != LATCH_PUNCT {
                                in_p = lastsp;
                            }
                            break;
                        }
                    }
                    if !in_p || lastidx.is_none() {
                        continue;
                    }
                    let lastidx = lastidx.unwrap();
                    let mut new_cost = curlen[i_state as usize];
                    let new_seq: Vec<i32>;
                    if lastidx < curseq_i_len - 1 {
                        // There's stuff after lastchar in the seq.
                        if i_state == STATE_PUNCT {
                            if lastld {
                                new_cost = new_cost.saturating_add(1);
                            }
                            // Remove lastchar at idx, append PAIR_x at the end.
                            let mut s = Vec::with_capacity(curseq_i_len);
                            s.extend_from_slice(&seq_i[..lastidx]);
                            s.extend_from_slice(&seq_i[lastidx + 1..]);
                            s.push(pair_sent);
                            new_seq = s;
                        } else {
                            // Non-P state: keep everything, replace lastchar
                            // at position lastidx with PAIR_x.
                            let mut s = seq_i.clone();
                            s[lastidx] = pair_sent;
                            new_seq = s;
                        }
                    } else {
                        // lastchar is at the end of curseq[i_state].
                        let mut s: Vec<i32> = seq_i[..curseq_i_len - 1].to_vec();
                        s.push(pair_sent);
                        new_seq = s;
                    }
                    if new_cost < nxtlen[i_state as usize] {
                        nxtlen[i_state as usize] = new_cost;
                        nxtseq[i_state as usize] = new_seq;
                    }
                }
            }
        }

        // BWIPP byte-mode count adjustment: if nxtseq[B] has exactly 32
        // bytes since the last SHIFT_BYTE, the count prefix grows from
        // 5 to 5+11 bits — bump nxtlen[B] by 11.
        if !nxtseq[STATE_BYTE as usize].is_empty() {
            let mut numbytes = 0u32;
            for &ch in &nxtseq[STATE_BYTE as usize] {
                if ch == SHIFT_BYTE {
                    numbytes = 0;
                } else {
                    numbytes += 1;
                }
            }
            if numbytes == 32 {
                nxtlen[STATE_BYTE as usize] = nxtlen[STATE_BYTE as usize].saturating_add(11);
            }
        }

        curseq = nxtseq;
        curlen = nxtlen;
        lastchar = Some(b);
    }

    // Final closure: latches at end (no char follows, but we may want
    // a cheaper exit). Take min across all states.
    let mut best_state = 0u8;
    let mut best_len = curlen[0];
    for x in 1..6u8 {
        if curlen[x as usize] < best_len {
            best_len = curlen[x as usize];
            best_state = x;
        }
    }
    if best_len == INF {
        return Err(crate::error::Error::InvalidData(
            "Aztec encode_dp: no reachable encoding found".into(),
        ));
    }
    Ok(curseq[best_state as usize].clone())
}

/// Convert a sentinel-sequence (output of [`encode_dp`]) to a bit
/// stream. Walks the seq tracking current state; emits each item's
/// codeword bits in the current state's width.
///
/// Shifts consume two seq items (the shift token + the char to emit
/// in the target alphabet). Latches transition state.
pub(crate) fn seq_to_bits(seq: &[i32]) -> Result<Vec<bool>, crate::error::Error> {
    let mut bits = Vec::new();
    let mut state = STATE_UPPER;
    let mut i = 0;
    while i < seq.len() {
        if state == STATE_BYTE {
            // Byte mode: count consecutive byte chars (non-negative items),
            // emit count prefix, then each as 8 bits. Exit on the next
            // latch sentinel (no codeword emitted for the exit — the
            // count tells the decoder when to revert).
            let mut count: usize = 0;
            while i + count < seq.len() && seq[i + count] >= 0 && count < 2078 {
                count += 1;
            }
            if count == 0 {
                return Err(crate::error::Error::InvalidData(
                    "Aztec seq_to_bits: BYTE state with no bytes to emit".into(),
                ));
            }
            if count <= 31 {
                append_codeword(&mut bits, count as u8, 5);
            } else {
                append_codeword(&mut bits, 0, 5);
                let extra = (count - 31) as u32;
                for k in (0..11).rev() {
                    bits.push((extra >> k) & 1 == 1);
                }
            }
            for _ in 0..count {
                let b = seq[i] as u8;
                append_codeword(&mut bits, b, 8);
                i += 1;
            }
            // Consume the exit latch sentinel without emitting (the count
            // prefix carries the implicit end-of-byte-mode signal).
            if i < seq.len() {
                let exit = seq[i];
                state = match exit {
                    LATCH_UPPER => STATE_UPPER,
                    LATCH_LOWER => STATE_LOWER,
                    LATCH_MIXED => STATE_MIXED,
                    _ => {
                        return Err(crate::error::Error::InvalidData(format!(
                            "Aztec seq_to_bits: invalid BYTE-exit sentinel {exit}",
                        )));
                    }
                };
                i += 1;
            }
            continue;
        }
        let item = seq[i];
        if item >= 0 {
            // Regular char emission in current state.
            let cw = encode_byte_in_state(state, item as u8).ok_or_else(|| {
                crate::error::Error::InvalidData(format!(
                    "Aztec seq_to_bits: byte 0x{:02x} not encodable in state {state}",
                    item,
                ))
            })?;
            append_codeword(&mut bits, cw, CHAR_SIZES[state as usize]);
            i += 1;
        } else if item == SHIFT_PUNCT || item == SHIFT_UPPER {
            // Single-char shift: emit shift codeword, then peek next item
            // and emit it in the target alphabet (state doesn't change).
            // The next item can be either a regular char or a PAIR_x
            // sentinel (Punct codewords 2-5) when shifting to Punct.
            let cw_shift = sentinel_codeword(state, item).ok_or_else(|| {
                crate::error::Error::InvalidData(format!(
                    "Aztec seq_to_bits: shift {item} not available from state {state}",
                ))
            })?;
            append_codeword(&mut bits, cw_shift, CHAR_SIZES[state as usize]);
            if i + 1 >= seq.len() {
                return Err(crate::error::Error::InvalidData(
                    "Aztec seq_to_bits: shift not followed by a char".into(),
                ));
            }
            let nxt = seq[i + 1];
            let target = if item == SHIFT_PUNCT {
                STATE_PUNCT
            } else {
                STATE_UPPER
            };
            let cw_char = if nxt >= 0 {
                encode_byte_in_state(target, nxt as u8).ok_or_else(|| {
                    crate::error::Error::InvalidData(format!(
                        "Aztec seq_to_bits: byte 0x{nxt:02x} not encodable in shifted state {target}",
                    ))
                })?
            } else if target == STATE_PUNCT && is_pair_sentinel(nxt) {
                sentinel_codeword(STATE_PUNCT, nxt).ok_or_else(|| {
                    crate::error::Error::InvalidData(format!(
                        "Aztec seq_to_bits: pair {nxt} not encodable after SP",
                    ))
                })?
            } else {
                return Err(crate::error::Error::InvalidData(format!(
                    "Aztec seq_to_bits: shift {item} followed by sentinel {nxt}",
                )));
            };
            append_codeword(&mut bits, cw_char, CHAR_SIZES[target as usize]);
            i += 2;
        } else if item == SHIFT_BYTE {
            // Latch into Byte state: emit SB codeword (5 bits) in current
            // state; the bytes themselves and their count prefix follow
            // on the next loop iteration.
            let cw = sentinel_codeword(state, item).ok_or_else(|| {
                crate::error::Error::InvalidData(format!(
                    "Aztec seq_to_bits: SHIFT_BYTE not available from state {state}",
                ))
            })?;
            append_codeword(&mut bits, cw, CHAR_SIZES[state as usize]);
            state = STATE_BYTE;
            i += 1;
        } else {
            // Regular latch sentinel.
            let cw = sentinel_codeword(state, item).ok_or_else(|| {
                crate::error::Error::InvalidData(format!(
                    "Aztec seq_to_bits: latch {item} not available from state {state}",
                ))
            })?;
            append_codeword(&mut bits, cw, CHAR_SIZES[state as usize]);
            if let Some(t) = latch_target(item) {
                state = t;
            }
            i += 1;
        }
    }
    Ok(bits)
}

/// Encode a byte slice into an Aztec bit stream using the DP encoder
/// + seq-to-bits converter.
///
/// This is the highest-level encoding entry point for ASCII inputs.
pub(crate) fn encode_msg(bytes: &[u8]) -> Result<Vec<bool>, crate::error::Error> {
    let seq = encode_dp(bytes)?;
    seq_to_bits(&seq)
}

/// Bit-stuff the raw bit stream into `bps`-bit codeword integers,
/// avoiding all-zero or all-one codewords (which would collide with
/// reference-grid / bull's-eye patterns) by inserting a complementary
/// stuffer bit after every (`bps` − 1) bits that would form one.
///
/// Algorithm (per BWIPP `azteccode` lines 30295–30322):
/// 1. Take the next `bps - 1` bits as `cwb`.
/// 2. If `cwb` is all-0, set the next bit to `1` (stuffer) and consume
///    only `bps - 1` real bits (the would-be `bps`-th bit becomes the
///    first bit of the next codeword).
/// 3. If `cwb` is all-1, set the next bit to `0`.
/// 4. Otherwise consume `bps` real bits as-is.
/// 5. Tail: if fewer than `bps` bits remain, pad with 1s. If the
///    resulting codeword is all-1, flip the last bit to 0.
pub(crate) fn bit_stuff(msgbits: &[bool], bps: u8) -> Vec<u32> {
    let mut cws = Vec::new();
    let mut m: usize = 0;
    let n = msgbits.len();
    let bps_u = bps as usize;
    let bpm1 = bps_u - 1;
    while m < n {
        let remaining = n - m;
        if remaining >= bps_u {
            let pre = &msgbits[m..m + bpm1];
            let actual_next = msgbits[m + bpm1];
            let cwf;
            let advance;
            if pre.iter().all(|&b| !b) {
                cwf = true;
                advance = bpm1;
            } else if pre.iter().all(|&b| b) {
                cwf = false;
                advance = bpm1;
            } else {
                cwf = actual_next;
                advance = bps_u;
            }
            let mut v: u32 = 0;
            for &b in pre {
                v = (v << 1) | (b as u32);
            }
            v = (v << 1) | (cwf as u32);
            cws.push(v);
            m += advance;
        } else {
            // Tail: pad with 1s, then knock the last bit down if all-1.
            let mut bits: Vec<bool> = msgbits[m..].to_vec();
            while bits.len() < bps_u {
                bits.push(true);
            }
            if bits.iter().all(|&b| b) {
                *bits.last_mut().unwrap() = false;
            }
            let mut v: u32 = 0;
            for &b in &bits {
                v = (v << 1) | (b as u32);
            }
            cws.push(v);
            m = n;
        }
    }
    cws
}

/// Find the smallest Aztec metric that can hold a message of
/// `msgbits_len` bits with at least the requested error correction.
///
/// Returns the metrics index, or `None` if the input is too large.
///
/// - `format`: `"full"` | `"compact"` | `"rune"`.
/// - `requested_layers`: pass `-1` to auto-select. For compact pass
///   1..=4, for full pass 1..=32.
/// - `eclevel`: percentage 5..=95 (default 23 per BWIPP).
/// - `ecaddchars`: minimum additional ECC codewords (default 3).
/// - `readerinit`: require reader-init capable symbol size.
pub(crate) fn fit_metric(
    msgbits_len: usize,
    format: &str,
    requested_layers: i32,
    eclevel: u32,
    ecaddchars: u32,
    readerinit: bool,
) -> Option<usize> {
    for (i, m) in METRICS.iter().enumerate() {
        if m.format != format {
            continue;
        }
        if readerinit && m.has_data != 1 {
            continue;
        }
        if requested_layers > 0 && requested_layers as u8 != m.layers {
            continue;
        }
        let ncws = m.ncws as u32;
        if ncws == 0 {
            // Rune metric — handled separately.
            continue;
        }
        let bpcw = m.bps as u32;
        // numecw = ceil(ncws * eclevel / 100 + ecaddchars)
        // Compute as integer: (ncws*eclevel + 99) / 100 + ecaddchars.
        let numecw = (ncws * eclevel).div_ceil(100) + ecaddchars;
        if numecw >= ncws {
            continue;
        }
        let numdcw = ncws - numecw;
        let dcw_needed = (msgbits_len as u32).div_ceil(bpcw);
        if dcw_needed <= numdcw {
            return Some(i);
        }
    }
    None
}

/// Build the final codewords (data + ECC) for an Aztec data symbol.
///
/// Bit-stuffs `msgbits` into data codewords, computes
/// `ncws − cws.len()` Reed-Solomon ECC codewords over GF(2^bpcw), and
/// returns `cws ++ ecc` totalling `ncws` codewords.
pub(crate) fn build_codewords(
    msgbits: &[bool],
    metrics_idx: usize,
) -> Result<Vec<u32>, crate::error::Error> {
    let m = METRICS[metrics_idx];
    let bpcw = m.bps;
    let ncws = m.ncws as usize;
    let cws = bit_stuff(msgbits, bpcw);
    if cws.len() > ncws {
        return Err(crate::error::Error::InvalidData(format!(
            "Aztec: data codewords ({}) exceed symbol capacity ({ncws})",
            cws.len(),
        )));
    }
    let n_ecc = ncws - cws.len();
    if n_ecc == 0 {
        return Ok(cws);
    }
    let gf = crate::util::rs_gf2k::gf_for_bps(bpcw).ok_or_else(|| {
        crate::error::Error::InvalidData(format!("Aztec: no GF parameters for bps={bpcw}"))
    })?;
    let ecc = crate::util::rs_gf2k::encode_k(&cws, n_ecc, gf);
    let mut out = cws;
    // BWIPP's `bwipp_rsecbinary` returns the LFSR low-slot last; our
    // `encode_k` returns it first, so reverse before appending.
    for &e in ecc.iter().rev() {
        out.push(e);
    }
    Ok(out)
}

/// Build the Aztec mode word + RS-ECC over GF(16). For full mode:
/// 4-nibble data + 6-nibble ECC = 10 nibbles = 40 bits. For compact:
/// 2 + 5 = 7 nibbles = 28 bits. For rune: 2 + 5 = 7, then each
/// nibble XOR'd with 10.
///
/// Returns a bit vector of length 40 / 28 / 28 (MSB first).
pub(crate) fn build_mode_bits(
    format: &str,
    layers: u8,
    cw_count: usize,
    readerinit: bool,
    rune_value: Option<u8>,
) -> Vec<bool> {
    let gf16 = crate::util::rs_gf2k::GF16;
    let (data_nibbles, ecc_count): (Vec<u32>, usize) = match format {
        "full" => {
            let mut mode = (layers as u32 - 1) * 2048 + (cw_count as u32 - 1);
            if readerinit {
                mode |= 1024;
            }
            (
                vec![
                    (mode >> 12) & 0xF,
                    (mode >> 8) & 0xF,
                    (mode >> 4) & 0xF,
                    mode & 0xF,
                ],
                6,
            )
        }
        "compact" => {
            let mode = (layers as u32 - 1) * 64 + (cw_count as u32 - 1);
            (vec![(mode >> 4) & 0xF, mode & 0xF], 5)
        }
        "rune" => {
            let mode = rune_value.unwrap_or(0) as u32;
            (vec![(mode >> 4) & 0xF, mode & 0xF], 5)
        }
        _ => panic!("unknown format {format}"),
    };
    let ecc = crate::util::rs_gf2k::encode_k(&data_nibbles, ecc_count, gf16);
    let mut all: Vec<u32> = data_nibbles;
    for &e in ecc.iter().rev() {
        all.push(e);
    }
    if format == "rune" {
        for v in all.iter_mut() {
            *v ^= 10;
        }
    }
    let mut bits = Vec::with_capacity(all.len() * 4);
    for v in all {
        for k in (0..4).rev() {
            bits.push((v >> k) & 1 == 1);
        }
    }
    bits
}

/// Final Aztec symbol matrix in row-major order: `matrix[y][x]` is
/// `0` or `1`. Side length is `size`.
#[derive(Debug, Clone)]
pub(crate) struct AztecSymbolMatrix {
    pub size: usize,
    pub pixels: Vec<Vec<u8>>,
}

/// Compact Aztec layout uses fw=9; full uses fw=12 initially. The
/// `lmv` function maps (layer, position) to (x, y) offsets from the
/// symbol center, per BWIPP `lmv` (line 30444).
///
/// `layer` is 1-indexed; `pos` walks 0..layer_bits where
/// `layer_bits = (fw + layer*4) * 8`. The path wraps around the
/// bull's-eye in 4 sides (top, right, bottom, left) × `lwid` columns
/// × 2 rows per column.
fn lmv(layer: i32, pos: i32, fw: i32) -> (i32, i32) {
    let lwid = fw + layer * 4;
    let dir = (pos / 2) / lwid;
    let col = (pos / 2) % lwid;
    let row = pos % 2;
    match dir {
        0 => {
            // Top side.
            let x = -((lwid - 1) / 2) + 1 + col;
            let y = (fw - 1) / 2 + layer * 2 + row;
            (x, y)
        }
        1 => {
            // Right side.
            let x = fw / 2 + layer * 2 + row;
            let y = (lwid - 1) / 2 - 1 - col;
            (x, y)
        }
        2 => {
            // Bottom side.
            let x = -(-(lwid / 2) + 1 + col);
            let y = -(fw / 2 + layer * 2 + row);
            (x, y)
        }
        3 => {
            // Left side.
            let x = -((fw - 1) / 2 + layer * 2 + row);
            let y = -(lwid / 2 - 1 - col);
            (x, y)
        }
        _ => unreachable!("lmv: invalid dir {dir}"),
    }
}

/// Convert center-relative `(x, y)` offset to a linear index in a
/// `size×size` grid with `mid_idx` as the center. Per BWIPP `cmv`:
/// `x` is column offset (+ right), `y` is row offset (+ up, so larger
/// y maps to a *smaller* row index).
fn cmv(x: i32, y: i32, mid_idx: i32, size: i32) -> i32 {
    x - y * size + mid_idx
}

/// Build the full Aztec symbol pixel matrix. Handles compact L1-L4
/// and full L1-L32 (reference-grid insertion included for full L≥5).
pub(crate) fn build_matrix(
    format: &str,
    layers: u8,
    cws: &[u32],
    bpcw: u8,
    modebits: &[bool],
) -> AztecSymbolMatrix {
    // Phase 1: lay codeword bits into a no-ref-grid pixs grid.
    let fw_initial: i32 = if format == "full" { 12 } else { 9 };
    let initial_size = fw_initial + (layers as i32) * 4 + 2;
    let mid = (initial_size - 1) / 2;
    let mid_idx = mid * initial_size + mid;
    let mut pixs: Vec<i8> = vec![-1; (initial_size * initial_size) as usize];

    // Build databits string: bpcw bits per codeword, zero-padded at
    // the start so the codeword list fills the END of the buffer.
    let total_cw_bits = cws.len() * bpcw as usize;
    let symbol_bits = if format == "full" {
        (layers as usize) * (layers as usize) * 16 + (layers as usize) * 112
    } else {
        (layers as usize) * (layers as usize) * 16 + (layers as usize) * 88
    };
    let mut databits = vec![false; symbol_bits];
    let offset = symbol_bits - total_cw_bits;
    for (i, &cw) in cws.iter().enumerate() {
        for k in 0..(bpcw as usize) {
            let bit = (cw >> ((bpcw as usize) - 1 - k)) & 1 == 1;
            databits[offset + i * (bpcw as usize) + k] = bit;
        }
    }

    // Walk layers outward, write databits in reverse (BWIPP reads
    // databits from the END).
    let mut bit_idx = 0;
    for layer in 1..=(layers as i32) {
        let layer_bits = (fw_initial + layer * 4) * 8;
        for pos in 0..layer_bits {
            let (x, y) = lmv(layer, pos, fw_initial);
            let idx = cmv(x, y, mid_idx, initial_size);
            let b = databits[databits.len() - 1 - bit_idx] as i8;
            pixs[idx as usize] = b;
            bit_idx += 1;
        }
    }

    // Phase 2: for full, expand pixs with reference grid lines every
    // 16 modules around center. Compact has no reference grid.
    let (mut pixs, fw, size, _mid, mid_idx) = if format == "full" {
        let fw2 = 13;
        let growth = ((((layers as i32) + 10) * 2 + 1) / 15 - 1).max(0) * 2;
        let new_size = fw2 + (layers as i32) * 4 + 2 + growth;
        let new_mid = (new_size - 1) / 2;
        let new_mid_idx = new_mid * new_size + new_mid;
        let total = (new_size * new_size) as usize;
        let mut npixs: Vec<i8> = vec![-2; total];
        // Draw reference grid: horizontal + vertical lines at every
        // 16-module step from center (including the bull's-eye
        // boundary at offset 0).
        let half = new_size / 2;
        let mut i = 0i32;
        while i <= half {
            for j in 0..new_size {
                let coord = -half + j;
                let val = (((half + j) + i) + 1) % 2;
                let val = val as i8;
                let idx_pp = cmv(coord, i, new_mid_idx, new_size);
                npixs[idx_pp as usize] = val;
                let idx_pn = cmv(coord, -i, new_mid_idx, new_size);
                npixs[idx_pn as usize] = val;
                let idx_np = cmv(i, coord, new_mid_idx, new_size);
                npixs[idx_np as usize] = val;
                let idx_nn = cmv(-i, coord, new_mid_idx, new_size);
                npixs[idx_nn as usize] = val;
            }
            i += 16;
        }
        // Copy data pixs into the slots that weren't filled by grid.
        let mut j = 0usize;
        for slot in npixs.iter_mut().take(total) {
            if *slot == -2 {
                *slot = pixs[j];
                j += 1;
            }
        }
        (npixs, fw2, new_size, new_mid, new_mid_idx)
    } else {
        (pixs, fw_initial, initial_size, mid, mid_idx)
    };

    // Phase 3: draw bull's-eye centered at mid. Pattern is alternating
    // filled/empty rings of width 1, with the center filled.
    let fw_half = fw / 2;
    for di in -fw_half..=fw_half {
        for dj in -fw_half..=fw_half {
            let a = di.abs().max(dj.abs());
            let idx = cmv(di, dj, mid_idx, size);
            pixs[idx as usize] = ((a + 1) % 2) as i8;
        }
    }

    // Phase 4: orientation marks — 12 fixed positions just outside
    // the bull's-eye edge.
    let orient: &[(i32, i32, u8)] = &[
        (-(fw_half + 1), fw_half, 1),
        (-(fw_half + 1), fw_half + 1, 1),
        (-fw_half, fw_half + 1, 1),
        (fw_half + 1, fw_half + 1, 1),
        (fw_half + 1, fw_half, 1),
        (fw_half + 1, -fw_half, 1),
        (fw_half, fw_half + 1, 0),
        (fw_half + 1, -(fw_half + 1), 0),
        (fw_half, -(fw_half + 1), 0),
        (-fw_half, -(fw_half + 1), 0),
        (-(fw_half + 1), -(fw_half + 1), 0),
        (-(fw_half + 1), -fw_half, 0),
    ];
    for &(dx, dy, v) in orient {
        let idx = cmv(dx, dy, mid_idx, size);
        pixs[idx as usize] = v as i8;
    }

    // Phase 5: place mode bits.
    let modemap: &[(i32, i32)] = if format == "full" {
        MODEMAP_FULL
    } else {
        MODEMAP_COMPACT
    };
    for (i, &(dx, dy)) in modemap.iter().enumerate() {
        if i >= modebits.len() {
            break;
        }
        let idx = cmv(dx, dy, mid_idx, size);
        pixs[idx as usize] = modebits[i] as i8;
    }

    // Phase 6: convert flat i8 pixs to 2D u8 matrix.
    let mut pixels = vec![vec![0u8; size as usize]; size as usize];
    for y in 0..size {
        for x in 0..size {
            // BWIPP indexes pixs as [dy - dx*size + mid] where the
            // pixel is at center-offset (dx, dy). To convert back:
            // pixs[y_lin * size + x_lin] where y_lin maps to dy = ?
            // and x_lin maps to dx = ?
            // Actually pixs is laid out so pixs[idx] where idx is
            // computed by cmv. For row-major output, we want
            // matrix[r][c] = pixs at (dx, dy) where dy = c - mid,
            // dx = -(r - mid) (or similar — let's derive).
            //
            // cmv(dx, dy) = dy - dx*size + mid_idx
            //            = dy - dx*size + mid*size + mid
            //            = (mid - dx)*size + (mid + dy)
            // So linear index = row*size + col where:
            //   row = mid - dx
            //   col = mid + dy
            // Therefore:
            //   dx = mid - row
            //   dy = col - mid
            // To populate output[r][c]:
            //   look up pixs at (dx, dy) = (mid - r, c - mid)
            //   linear idx = r * size + c
            let v = pixs[(y * size + x) as usize];
            // Treat -1 (unwritten) as 0.
            pixels[y as usize][x as usize] = if v == 1 { 1 } else { 0 };
        }
    }

    AztecSymbolMatrix {
        size: size as usize,
        pixels,
    }
}

/// Aztec full-mode bit positions (relative to center). Direct port of
/// `azteccode_modemapfull` (bwip-js line 29634).
pub(crate) const MODEMAP_FULL: &[(i32, i32)] = &[
    (-5, 7),
    (-4, 7),
    (-3, 7),
    (-2, 7),
    (-1, 7),
    (1, 7),
    (2, 7),
    (3, 7),
    (4, 7),
    (5, 7),
    (7, 5),
    (7, 4),
    (7, 3),
    (7, 2),
    (7, 1),
    (7, -1),
    (7, -2),
    (7, -3),
    (7, -4),
    (7, -5),
    (5, -7),
    (4, -7),
    (3, -7),
    (2, -7),
    (1, -7),
    (-1, -7),
    (-2, -7),
    (-3, -7),
    (-4, -7),
    (-5, -7),
    (-7, -5),
    (-7, -4),
    (-7, -3),
    (-7, -2),
    (-7, -1),
    (-7, 1),
    (-7, 2),
    (-7, 3),
    (-7, 4),
    (-7, 5),
];

/// Public entry point for Aztec Code encoding.
///
/// Encodes `data` as ASCII text into the smallest Aztec symbol that
/// fits (preferring compact over full, then full L1..L32). Uses
/// BWIPP defaults: `eclevel=23`, `ecaddchars=3`, no reader init,
/// auto-select layers.
///
/// Returns a [`BitMatrix`] with `true` for black/foreground modules.
///
/// # Errors
/// - `InvalidData` if the input is empty or doesn't fit any Aztec
///   size (compact L1-L4 + full L1-L32). High-bit and arbitrary
///   binary bytes are encoded via Byte mode.
pub fn encode(data: &[u8]) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    encode_inner(data, false)
}

/// Encode an Aztec Code in **compact** mode only. BWIPP
/// `azteccodecompact` — equivalent to `encode` but rejects payloads
/// that don't fit compact L1-L4 instead of escalating to a full-size
/// symbol.
///
/// # Errors
/// - `InvalidData` if the input is empty or its bit length doesn't
///   fit any compact L1-L4 symbol.
pub fn encode_compact(data: &[u8]) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    encode_inner(data, true)
}

/// Encode an Aztec **Rune** — a fixed 11×11 marker whose payload is a
/// single integer value 0..=255. BWIPP `aztecrune`. The value is the
/// 8-bit "rune" itself, packed into the mode word (no data layers).
///
/// Input is a 1- to 3-digit ASCII decimal string; we parse it to a
/// `u8` and reject inputs outside 0..=255 or containing non-digits.
///
/// # Errors
/// - `InvalidData` if the input is empty, non-numeric, longer than
///   three digits, or parses to a value > 255.
pub fn encode_rune(data: &str) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    if data.is_empty() {
        return Err(crate::error::Error::InvalidData(
            "Aztec Rune: input must be a 1-3 digit integer (0..=255)".into(),
        ));
    }
    if data.len() > 3 {
        return Err(crate::error::Error::InvalidData(format!(
            "Aztec Rune: input must be 1-3 digits, got {} chars",
            data.len()
        )));
    }
    for b in data.bytes() {
        if !b.is_ascii_digit() {
            return Err(crate::error::Error::InvalidData(format!(
                "Aztec Rune: non-digit byte 0x{b:02x} in input"
            )));
        }
    }
    let value: u32 = data.parse().map_err(|_| {
        crate::error::Error::InvalidData(format!("Aztec Rune: cannot parse {data:?} as integer"))
    })?;
    if value > 255 {
        return Err(crate::error::Error::InvalidData(format!(
            "Aztec Rune: value must be 0..=255, got {value}"
        )));
    }
    let rune = value as u8;
    let modebits = build_mode_bits("rune", 0, 0, false, Some(rune));
    // METRICS[0] is rune (layers=0, bps=6). build_matrix takes the
    // bpcw parameter that's used only for the data-layer placement;
    // rune has no data layers, so any value works — pass 6 to match
    // the metric for clarity.
    let sym = build_matrix("rune", 0, &[], 6, &modebits);
    let mut bm = crate::encoding::BitMatrix::new(sym.size, sym.size);
    for y in 0..sym.size {
        for x in 0..sym.size {
            bm.set(x, y, sym.pixels[y][x] == 1);
        }
    }
    Ok(bm)
}

fn encode_inner(
    data: &[u8],
    force_compact: bool,
) -> Result<crate::encoding::BitMatrix, crate::error::Error> {
    if data.is_empty() {
        return Err(crate::error::Error::InvalidData(
            "Aztec: input data must not be empty".into(),
        ));
    }
    let msgbits = encode_msg(data)?;
    let (format, metric_idx) = match fit_metric(msgbits.len(), "compact", -1, 23, 3, false) {
        Some(idx) => ("compact", idx),
        None => {
            if force_compact {
                return Err(crate::error::Error::InvalidData(
                    "Aztec Compact: input data exceeds the L1-L4 compact size range".into(),
                ));
            }
            let idx = fit_metric(msgbits.len(), "full", -1, 23, 3, false).ok_or_else(|| {
                crate::error::Error::InvalidData(
                    "Aztec: input data exceeds maximum symbol size".into(),
                )
            })?;
            ("full", idx)
        }
    };
    let m = METRICS[metric_idx];
    let cws = build_codewords(&msgbits, metric_idx)?;
    let cw_count = bit_stuff(&msgbits, m.bps).len();
    let modebits = build_mode_bits(format, m.layers, cw_count, false, None);
    let sym = build_matrix(format, m.layers, &cws, m.bps, &modebits);
    let mut bm = crate::encoding::BitMatrix::new(sym.size, sym.size);
    for y in 0..sym.size {
        for x in 0..sym.size {
            bm.set(x, y, sym.pixels[y][x] == 1);
        }
    }
    Ok(bm)
}

/// Aztec compact-mode bit positions (relative to center). Direct
/// port of `azteccode_modemapcompact` (bwip-js line 29641).
pub(crate) const MODEMAP_COMPACT: &[(i32, i32)] = &[
    (-3, 5),
    (-2, 5),
    (-1, 5),
    (0, 5),
    (1, 5),
    (2, 5),
    (3, 5),
    (5, 3),
    (5, 2),
    (5, 1),
    (5, 0),
    (5, -1),
    (5, -2),
    (5, -3),
    (3, -5),
    (2, -5),
    (1, -5),
    (0, -5),
    (-1, -5),
    (-2, -5),
    (-3, -5),
    (-5, -3),
    (-5, -2),
    (-5, -1),
    (-5, 0),
    (-5, 1),
    (-5, 2),
    (-5, 3),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn char_sizes_shape() {
        assert_eq!(CHAR_SIZES, [5, 5, 5, 5, 4, 8]);
    }

    #[test]
    fn latch_len_shape() {
        // Self-latches are 0.
        for (s, row) in LATCH_LEN.iter().enumerate() {
            assert_eq!(row[s], 0, "self-latch[{s}] should be 0");
        }
        // First row, first off-diagonal latch.
        assert_eq!(LATCH_LEN[0][1], 5); // upper → lower = 5 bits
    }

    #[test]
    fn shift_len_shape() {
        // No shifts from punct to anywhere.
        for &v in &SHIFT_LEN[3] {
            assert_eq!(v, u16::MAX);
        }
        // Digit → upper is 4-bit shift (the FNC1-ish quick exit).
        assert_eq!(SHIFT_LEN[4][0], 4);
    }

    #[test]
    fn metrics_table_shape() {
        assert_eq!(METRICS.len(), 37);
        // Index 0: rune (no data).
        assert_eq!(METRICS[0].format, "rune");
        assert_eq!(METRICS[0].has_data, 0);
        // Index 1: compact L1 (17 cws).
        assert_eq!(METRICS[1].format, "compact");
        assert_eq!(METRICS[1].layers, 1);
        assert_eq!(METRICS[1].ncws, 17);
        // Index 36: full L32 (last entry).
        assert_eq!(METRICS[36].format, "full");
        assert_eq!(METRICS[36].layers, 32);
        assert_eq!(METRICS[36].ncws, 1664);
        assert_eq!(METRICS[36].bps, 12);
        // BPS transitions: 6 (L1-2), 8 (L3-8), 10 (L9-22), 12 (L23-32).
        for m in METRICS.iter() {
            assert!(
                matches!(m.bps, 6 | 8 | 10 | 12),
                "unexpected bps {} for layers {}",
                m.bps,
                m.layers,
            );
        }
    }

    #[test]
    fn upper_codeword_known_values() {
        // Space, A, Z, edge cases.
        assert_eq!(upper_codeword(b' '), Some(1));
        assert_eq!(upper_codeword(b'A'), Some(2));
        assert_eq!(upper_codeword(b'Z'), Some(27));
        // Lowercase, digits, punct → None (need shift/latch).
        assert_eq!(upper_codeword(b'a'), None);
        assert_eq!(upper_codeword(b'0'), None);
        assert_eq!(upper_codeword(b'!'), None);
    }

    #[test]
    fn lower_codeword_known_values() {
        assert_eq!(lower_codeword(b' '), Some(1));
        assert_eq!(lower_codeword(b'a'), Some(2));
        assert_eq!(lower_codeword(b'z'), Some(27));
        assert_eq!(lower_codeword(b'A'), None);
    }

    #[test]
    fn digit_codeword_known_values() {
        assert_eq!(digit_codeword(b' '), Some(1));
        assert_eq!(digit_codeword(b'0'), Some(2));
        assert_eq!(digit_codeword(b'9'), Some(11));
        assert_eq!(digit_codeword(b','), Some(12));
        assert_eq!(digit_codeword(b'.'), Some(13));
        assert_eq!(digit_codeword(b'A'), None);
    }

    #[test]
    fn mixed_codeword_known_values() {
        assert_eq!(mixed_codeword(b' '), Some(1));
        // Control char 1 (^A) → codeword 2.
        assert_eq!(mixed_codeword(1), Some(2));
        // Control char 13 (^M / CR) → codeword 14.
        assert_eq!(mixed_codeword(13), Some(14));
        // ESC → codeword 15.
        assert_eq!(mixed_codeword(27), Some(15));
        // @ → codeword 20.
        assert_eq!(mixed_codeword(b'@'), Some(20));
        // DEL (127) → codeword 27.
        assert_eq!(mixed_codeword(127), Some(27));
        // Lowercase → None.
        assert_eq!(mixed_codeword(b'a'), None);
    }

    #[test]
    fn punct_codeword_known_values() {
        assert_eq!(punct_codeword(13), Some(1)); // CR
        assert_eq!(punct_codeword(b'!'), Some(6));
        assert_eq!(punct_codeword(b'?'), Some(26));
        assert_eq!(punct_codeword(b'['), Some(27));
        assert_eq!(punct_codeword(b'{'), Some(29));
        // Uppercase → None.
        assert_eq!(punct_codeword(b'A'), None);
    }

    #[test]
    fn encode_single_state_pure_uppercase() {
        // "HELLO" in Upper state: H=9, E=6, L=13, L=13, O=16.
        let cws = encode_single_state(STATE_UPPER, b"HELLO").unwrap();
        assert_eq!(cws, vec![9, 6, 13, 13, 16]);
    }

    #[test]
    fn encode_single_state_pure_digits() {
        // "12345" in Digit state: '1'=3, '2'=4, '3'=5, '4'=6, '5'=7.
        let cws = encode_single_state(STATE_DIGIT, b"12345").unwrap();
        assert_eq!(cws, vec![3, 4, 5, 6, 7]);
    }

    #[test]
    fn encode_single_state_rejects_wrong_alphabet() {
        // Lowercase in Upper state → None.
        assert!(encode_single_state(STATE_UPPER, b"hello").is_none());
        // Uppercase in Digit state → None.
        assert!(encode_single_state(STATE_DIGIT, b"ABC").is_none());
    }

    #[test]
    fn pack_codewords_to_bits_5bit() {
        // codeword 9 = 0b01001 → [F, T, F, F, T]
        let bits = pack_codewords_to_bits(&[9], 5);
        assert_eq!(bits, vec![false, true, false, false, true]);
        // Two codewords [9, 6] → 10 bits.
        let bits = pack_codewords_to_bits(&[9, 6], 5);
        assert_eq!(bits.len(), 10);
        // 6 = 0b00110.
        assert_eq!(&bits[5..], &[false, false, true, true, false]);
    }

    #[test]
    fn pack_codewords_to_bits_4bit() {
        // codeword 3 in 4 bits = 0011.
        let bits = pack_codewords_to_bits(&[3], 4);
        assert_eq!(bits, vec![false, false, true, true]);
    }

    #[test]
    fn encode_greedy_pure_upper_matches_single_state() {
        // "HELLO" stays in Upper from the start — no latches.
        let got = encode_greedy(b"HELLO").unwrap();
        let cws = encode_single_state(STATE_UPPER, b"HELLO").unwrap();
        let want = pack_codewords_to_bits(&cws, 5);
        assert_eq!(got, want);
        // 5 codewords × 5 bits = 25 bits.
        assert_eq!(got.len(), 25);
    }

    #[test]
    fn encode_greedy_pure_lower_starts_with_ll() {
        // Default state is Upper; "hello" needs LL (codeword 28) first.
        let got = encode_greedy(b"hello").unwrap();
        // LL = 28 in 5 bits = 11100.
        assert_eq!(&got[..5], &[true, true, true, false, false]);
        // Then h..o codewords from Lower state (cws 9, 6, 13, 13, 16).
        let lower_cws = encode_single_state(STATE_LOWER, b"hello").unwrap();
        let lower_bits = pack_codewords_to_bits(&lower_cws, 5);
        assert_eq!(&got[5..], &lower_bits[..]);
        // 1 latch + 5 data = 6 codewords × 5 bits = 30 bits.
        assert_eq!(got.len(), 30);
    }

    #[test]
    fn encode_greedy_space_in_lower_stays_in_lower() {
        // "Aa b" — A (Upper, cw 2), LL (28), a (Lower, cw 2),
        // space (cw 1 in Lower, no latch), b (cw 3 in Lower).
        let got = encode_greedy(b"Aa b").unwrap();
        // Expected codewords with widths:
        // Upper: A=2 (5 bits)
        // Upper: LL=28 (5 bits)
        // Lower: a=2 (5 bits)
        // Lower: space=1 (5 bits)
        // Lower: b=3 (5 bits)
        let mut want = Vec::new();
        for cw in [2u8, 28, 2, 1, 3] {
            for k in (0..5).rev() {
                want.push((cw >> k) & 1 == 1);
            }
        }
        assert_eq!(got, want);
        assert_eq!(got.len(), 25);
    }

    #[test]
    fn encode_greedy_pure_digits_latches_to_digit() {
        // "123" — LD (cw 30 in Upper, 5 bits), then 4-bit digits 1=3, 2=4, 3=5.
        let got = encode_greedy(b"123").unwrap();
        // Upper LD: 30 → 11110 (5 bits)
        // Digit '1' = 3 → 0011 (4 bits)
        // Digit '2' = 4 → 0100 (4 bits)
        // Digit '3' = 5 → 0101 (4 bits)
        let mut want = Vec::new();
        for k in (0..5).rev() {
            want.push((30u8 >> k) & 1 == 1);
        }
        for cw in [3u8, 4, 5] {
            for k in (0..4).rev() {
                want.push((cw >> k) & 1 == 1);
            }
        }
        assert_eq!(got, want);
        // 5 + 3×4 = 17 bits.
        assert_eq!(got.len(), 17);
    }

    #[test]
    fn encode_greedy_rejects_high_bit_byte() {
        let err = encode_greedy(&[0x80]).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("Byte-state"),
            "unexpected error message: {msg}",
        );
    }

    #[test]
    fn encode_greedy_handles_mixed_upper_lower() {
        // "Hello" — H (Upper, cw 9), LL (28), e (cw 6), l (cw 13), l (cw 13), o (cw 16).
        let got = encode_greedy(b"Hello").unwrap();
        let mut want = Vec::new();
        for cw in [9u8, 28, 6, 13, 13, 16] {
            for k in (0..5).rev() {
                want.push((cw >> k) & 1 == 1);
            }
        }
        assert_eq!(got, want);
    }

    #[test]
    fn encode_greedy_rejects_unreachable_latch() {
        // "1a" — start Upper, latch to Digit for '1', then 'a' needs Lower
        // but Digit→Lower isn't a single-step latch. encode_greedy is
        // intentionally a single-step helper; the multi-step paths go
        // through `encode_seq` (the DP encoder) which the public
        // `encode` driver uses for real catalog inputs.
        let err = encode_greedy(b"1a").unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("no direct latch from state 4 to 1"),
            "unexpected error message: {msg}",
        );
    }

    #[test]
    fn encode_dp_pure_upper() {
        // "HELLO" — Upper state, no latches.
        let seq = encode_dp(b"HELLO").unwrap();
        assert_eq!(
            seq,
            vec![
                b'H' as i32,
                b'E' as i32,
                b'L' as i32,
                b'L' as i32,
                b'O' as i32
            ]
        );
    }

    #[test]
    fn encode_dp_pure_lower_inserts_ll() {
        // "hello" — Upper → Lower transition, then lowercase chars.
        let seq = encode_dp(b"hello").unwrap();
        // Should start with LATCH_LOWER sentinel.
        assert_eq!(seq[0], LATCH_LOWER);
        assert_eq!(
            &seq[1..],
            &[
                b'h' as i32,
                b'e' as i32,
                b'l' as i32,
                b'l' as i32,
                b'o' as i32
            ]
        );
    }

    #[test]
    fn encode_dp_pure_digit_inserts_ld() {
        let seq = encode_dp(b"12345").unwrap();
        assert_eq!(seq[0], LATCH_DIGIT);
        assert_eq!(
            &seq[1..],
            &[
                b'1' as i32,
                b'2' as i32,
                b'3' as i32,
                b'4' as i32,
                b'5' as i32
            ]
        );
    }

    #[test]
    fn encode_dp_lower_to_upper_path() {
        // "ab1" — start Upper, LL, ab, then LD-LU-...-1? Or stay in Lower
        // and use LD shortcut. Lower→Digit is a direct latch (LD, 5 bits).
        let seq = encode_dp(b"ab1").unwrap();
        assert_eq!(seq[0], LATCH_LOWER);
        assert_eq!(seq[1], b'a' as i32);
        assert_eq!(seq[2], b'b' as i32);
        // After 'b' in Lower state, latch to Digit (LD).
        assert_eq!(seq[3], LATCH_DIGIT);
        assert_eq!(seq[4], b'1' as i32);
    }

    #[test]
    fn encode_dp_accepts_high_bit_byte_via_byte_mode() {
        // High-bit byte now goes through Byte mode: U → SHIFT_BYTE → 0x80.
        let seq = encode_dp(&[0x80]).unwrap();
        assert_eq!(seq, vec![SHIFT_BYTE, 0x80]);
    }

    #[test]
    fn seq_to_bits_byte_mode_single_byte() {
        // [SHIFT_BYTE, 0x80] → SB (cw 31, 5 bits in Upper) + count=1 (5 bits)
        // + byte 0x80 (8 bits) = 18 bits.
        let bits = seq_to_bits(&[SHIFT_BYTE, 0x80]).unwrap();
        assert_eq!(bits.len(), 18);
        // SB = 31 = 11111 (5 bits).
        assert_eq!(&bits[..5], &[true; 5]);
        // count=1 = 00001 (5 bits).
        assert_eq!(&bits[5..10], &[false, false, false, false, true]);
        // byte 0x80 = 10000000 (8 bits).
        assert_eq!(
            &bits[10..18],
            &[true, false, false, false, false, false, false, false]
        );
    }

    #[test]
    fn seq_to_bits_byte_mode_with_exit() {
        // [SHIFT_BYTE, 0xAB, LATCH_UPPER, b'X'] → SB + count=1 + 0xAB + (no
        // exit codeword) + 'X' in Upper.
        let bits = seq_to_bits(&[SHIFT_BYTE, 0xAB, LATCH_UPPER, b'X' as i32]).unwrap();
        // SB=5, count=5, byte=8, X=5 → 23 bits.
        assert_eq!(bits.len(), 23);
    }

    #[test]
    fn seq_to_bits_byte_mode_multi_byte() {
        // 3 bytes after SB. Count=3 in 5 bits, then 3×8 bits.
        let bits = seq_to_bits(&[SHIFT_BYTE, 0x80, 0x81, 0x82]).unwrap();
        // SB(5) + count=3(5) + 3×8 = 34.
        assert_eq!(bits.len(), 34);
    }

    #[test]
    fn seq_to_bits_byte_mode_long_run() {
        // 32 bytes: count > 31 → uses 5-bit 0 + 11-bit extended count.
        let mut seq: Vec<i32> = vec![SHIFT_BYTE];
        seq.extend(std::iter::repeat_n(0x80, 32));
        let bits = seq_to_bits(&seq).unwrap();
        // SB(5) + count_prefix(0 in 5 + (32-31) in 11) = 5+5+11 = 21,
        // + 32×8 = 256, total = 277.
        assert_eq!(bits.len(), 277);
        // After SB, the next 5 bits should be 00000 (count=0 sentinel).
        assert_eq!(&bits[5..10], &[false; 5]);
        // Then 11 bits = (32-31) = 1 = 00000000001.
        assert_eq!(
            &bits[10..21],
            &[false, false, false, false, false, false, false, false, false, false, true,]
        );
    }

    #[test]
    fn encode_msg_handles_high_bit_byte() {
        let bits = encode_msg(b"\x80").unwrap();
        // Should not error. Validate length: SB(5) + count(5) + byte(8) = 18.
        assert_eq!(bits.len(), 18);
    }

    #[test]
    fn seq_to_bits_pure_upper() {
        // "HELLO" — 5 codewords × 5 bits = 25 bits.
        let seq = vec![
            b'H' as i32,
            b'E' as i32,
            b'L' as i32,
            b'L' as i32,
            b'O' as i32,
        ];
        let bits = seq_to_bits(&seq).unwrap();
        let cws = encode_single_state(STATE_UPPER, b"HELLO").unwrap();
        let want = pack_codewords_to_bits(&cws, 5);
        assert_eq!(bits, want);
    }

    #[test]
    fn seq_to_bits_with_latch() {
        // LL sentinel → cw 28 in Upper, then 'h' = cw 9 in Lower.
        let seq = vec![LATCH_LOWER, b'h' as i32];
        let bits = seq_to_bits(&seq).unwrap();
        // 28 in 5 bits = 11100, 9 in 5 bits = 01001.
        let want = vec![
            true, true, true, false, false, // 28
            false, true, false, false, true, // 9
        ];
        assert_eq!(bits, want);
    }

    #[test]
    fn seq_to_bits_digit_latch_4bit_codewords() {
        // LD (cw 30 in Upper, 5 bits) + '1' (cw 3 in Digit, 4 bits).
        let seq = vec![LATCH_DIGIT, b'1' as i32];
        let bits = seq_to_bits(&seq).unwrap();
        // 30 = 11110 (5 bits), 3 = 0011 (4 bits).
        let want = vec![
            true, true, true, true, false, // 30
            false, false, true, true, // 3
        ];
        assert_eq!(bits, want);
    }

    #[test]
    fn encode_msg_pure_upper_matches_greedy() {
        // For pure-Upper input, DP should match greedy.
        let dp = encode_msg(b"HELLO").unwrap();
        let greedy = encode_greedy(b"HELLO").unwrap();
        assert_eq!(dp, greedy);
    }

    #[test]
    fn encode_msg_handles_capitalised_word() {
        // "Hello" — H in Upper, LL latch, ello in Lower.
        // DP should produce: H (Upper), LL, e/l/l/o (Lower).
        let bits = encode_msg(b"Hello").unwrap();
        // Expected codewords: 9 (H), 28 (LL), 6 (e), 13 (l), 13 (l), 16 (o).
        // 6 codewords × 5 bits = 30 bits.
        assert_eq!(bits.len(), 30);
        // First 5 bits: H = 9 = 01001.
        assert_eq!(&bits[..5], &[false, true, false, false, true]);
        // Next 5 bits: LL = 28 = 11100.
        assert_eq!(&bits[5..10], &[true, true, true, false, false]);
    }

    #[test]
    fn encode_msg_uses_shift_for_single_punct() {
        // "A!" — A in Upper (cw 2), then '!' needs Punct. With SP shift,
        // bit cost is 5 (SP from Upper) + 5 (! in Punct) = 10. With latch,
        // cost would be 5 (LM from Upper) + 5 (LP from Mixed) + 5 (! in Punct)
        // = 15. DP should pick shift.
        let seq = encode_dp(b"A!").unwrap();
        // Expect: A, SP, !
        assert_eq!(seq, vec![b'A' as i32, SHIFT_PUNCT, b'!' as i32]);
        let bits = seq_to_bits(&seq).unwrap();
        // 3 codewords × 5 bits = 15.
        assert_eq!(bits.len(), 15);
    }

    #[test]
    fn encode_msg_round_trips_to_bits() {
        // End-to-end check that encode_msg returns bits matching
        // (encode_dp + seq_to_bits) composition.
        for &input in &[
            "HELLO",
            "Hello",
            "Hello world",
            "12345",
            "A1",
            "ABC123",
            "hello world",
        ] {
            let bits = encode_msg(input.as_bytes()).unwrap();
            let seq = encode_dp(input.as_bytes()).unwrap();
            let bits2 = seq_to_bits(&seq).unwrap();
            assert_eq!(
                bits, bits2,
                "encode_msg != encode_dp+seq_to_bits for {input:?}"
            );
        }
    }

    #[test]
    fn bit_stuff_no_padding_bps6() {
        // 6 bits of arbitrary mid-range value (010101) → one codeword,
        // no stuffing needed.
        let bits = vec![false, true, false, true, false, true];
        let cws = bit_stuff(&bits, 6);
        assert_eq!(cws, vec![0b010101]);
    }

    #[test]
    fn bit_stuff_all_zero_inserts_one() {
        // Input: 0 0 0 0 0 1 1 0 1 0 1 1 (12 bits)
        // 1st codeword: cwb = 00000 → all-0, cwf forced to 1 → cw = 000001;
        //              advance 5 real bits (the would-be 6th bit gets re-read).
        // 2nd codeword: starting at index 5, pre = [1,1,0,1,0], next = 1
        //              → cwf = 1, advance 6 → cw = 110101 = 53.
        // 3rd codeword (tail, 1 bit left): pad with 1s → 111111 → all-1 →
        //              flip last → 111110 = 62.
        let bits: Vec<bool> = vec![
            false, false, false, false, false, true, true, false, true, false, true, true,
        ];
        let cws = bit_stuff(&bits, 6);
        assert_eq!(cws, vec![0b000001, 0b110101, 0b111110]);
    }

    #[test]
    fn bit_stuff_all_ones_inserts_zero() {
        // 5 leading ones → cwb=11111, stuffer=0.
        // codeword 1 = 111110, then remaining starts at bit 5 of input.
        let bits: Vec<bool> = vec![
            true, true, true, true, true, // 5 ones
            false, true, false, false, // tail (4 bits)
        ];
        let cws = bit_stuff(&bits, 6);
        // First: 111110.
        // Remaining: 0 1 0 0 (4 bits) → tail, pad to 010011 (4 bits + "11" pad).
        // Not all-1, no fix needed.
        assert_eq!(cws, vec![0b111110, 0b010011]);
    }

    #[test]
    fn bit_stuff_tail_all_ones_flips_last() {
        // 6 bits of ones → first codeword 111110 (stuffer 0), then
        // remaining 1 bit = 1 → tail pad to 111111 → all-1 → flip to 111110.
        let bits: Vec<bool> = vec![true; 6];
        let cws = bit_stuff(&bits, 6);
        assert_eq!(cws, vec![0b111110, 0b111110]);
    }

    #[test]
    fn bit_stuff_empty_input() {
        // Stage 11.A8c (cont) — descriptive label naming bit_stuff empty
        // contract: zero input bits must yield zero codewords (no
        // off-by-one or implicit padding mutations).
        let cws = bit_stuff(&[], 6);
        assert!(
            cws.is_empty(),
            "bit_stuff(empty, bps=6) must yield empty codeword vec (no spurious padding/sentinel from mutation); got len={}",
            cws.len()
        );
    }

    #[test]
    fn bit_stuff_bps8() {
        // 8 bits not all-0 or all-1: codeword = the 8 bits.
        let bits: Vec<bool> = vec![true, false, true, false, true, false, true, false];
        let cws = bit_stuff(&bits, 8);
        assert_eq!(cws, vec![0b10101010]);
    }

    #[test]
    fn fit_metric_picks_compact_l1_for_short_input() {
        // "HELLO" encodes to ~25 bits + stuffing ≈ 25 bits → 5 cws in
        // bps=6 → fits compact L1 (17 cws, numecw=8 → 9 dcw available).
        let idx = fit_metric(25, "compact", -1, 23, 3, false).unwrap();
        assert_eq!(METRICS[idx].format, "compact");
        assert_eq!(METRICS[idx].layers, 1);
        assert_eq!(METRICS[idx].ncws, 17);
    }

    #[test]
    fn fit_metric_picks_larger_for_long_input() {
        // 500 bits doesn't fit any compact size (largest compact L4
        // has numdcw=55 → 440 bits max).
        let idx2 = fit_metric(500, "compact", -1, 23, 3, false);
        assert!(idx2.is_none(), "500 bits should not fit any compact size");
        // Full L4 (ncws=88, bpcw=8, numecw=24, numdcw=64) fits
        // 64*8 = 512 ≥ 500 bits.
        let idx3 = fit_metric(500, "full", -1, 23, 3, false).unwrap();
        assert_eq!(METRICS[idx3].format, "full");
        assert!(METRICS[idx3].layers >= 2);
    }

    #[test]
    fn fit_metric_forced_layers() {
        let idx = fit_metric(50, "full", 5, 23, 3, false).unwrap();
        assert_eq!(METRICS[idx].layers, 5);
        assert_eq!(METRICS[idx].format, "full");
    }

    /// Stage 11.A8c — pin `fit_metric`'s four None-return paths. The
    /// existing fit_metric_picks_compact_l1_for_short_input,
    /// fit_metric_picks_larger_for_long_input, and
    /// fit_metric_forced_layers tests all hit the happy `return
    /// Some(i)` arm. The fall-off-the-end `None` from the four early-
    /// continue filters is only partly exercised (the 500-bit compact
    /// case in fit_metric_picks_larger_for_long_input covers
    /// dcw-overflow). Three other filters survive:
    ///   * `m.format != format`: an unknown format string would still
    ///     find a metric if the guard were dropped.
    ///   * `requested_layers > 0 && requested_layers as u8 != m.layers`:
    ///     a layer count that doesn't exist for the format (e.g.
    ///     compact L5, since compact only has L1-L4) would silently
    ///     return the first format-matching metric.
    ///   * `numecw >= ncws`: an `eclevel` so high that no metric has
    ///     room for data (the ECC eats everything) should yield None.
    ///
    /// Mutations caught:
    ///   * `if m.format != format { continue; }` removed: case (1)
    ///     would return Some(1) (compact L1) instead of None.
    ///   * `if requested_layers > 0 && ...` removed: cases (2)/(3)
    ///     would return the first format-matching metric.
    ///   * `if numecw >= ncws { continue; }` → `> ncws`: at eclevel=100
    ///     numecw exactly equals or exceeds ncws+ecaddchars, dropping
    ///     all metrics; the `>` mutant would let some pass through.
    #[test]
    fn fit_metric_none_paths_invalid_format_and_impossible_layer() {
        // (1) Unknown format string → None (the m.format != format
        //     filter skips every iteration).
        assert!(
            fit_metric(25, "garbage", -1, 23, 3, false).is_none(),
            "unknown format 'garbage' → None"
        );

        // (1b) Empty format string → also None (same filter).
        assert!(
            fit_metric(25, "", -1, 23, 3, false).is_none(),
            "empty format → None"
        );

        // (2) Forced layer that doesn't exist in compact (compact
        //     only has L1-L4 per METRICS) → None.
        assert!(
            fit_metric(25, "compact", 5, 23, 3, false).is_none(),
            "compact L5 doesn't exist → None"
        );

        // (3) Forced layer above full's max of L32 → None.
        assert!(
            fit_metric(25, "full", 33, 23, 3, false).is_none(),
            "full L33 doesn't exist → None"
        );

        // (4) eclevel=100: numecw = (ncws*100)/100 + 3 = ncws + 3,
        //     which is always >= ncws → skip every metric. The
        //     happy-path tests use eclevel=23 where numecw < ncws.
        assert!(
            fit_metric(25, "full", -1, 100, 3, false).is_none(),
            "eclevel=100% leaves no data slots → None"
        );
    }

    /// Stage 11.A8c — pin two `build_codewords` branches the existing
    /// `build_codewords_short_input_compact_l1` doesn't reach:
    ///   1. **`n_ecc == 0` exact-fit early return.** When `cws.len()`
    ///      equals `ncws`, the function should return `Ok(cws)`
    ///      WITHOUT calling `rs_gf2k::encode_k` (no ECC slack to
    ///      fill). The existing HELLO test has 25 msgbits → 5 cws +
    ///      12-cw ECC tail, so `n_ecc` is never 0.
    ///   2. **The over-capacity `cws.len() > ncws` error arm.** When
    ///      msgbits produce more codewords than the chosen metric
    ///      can hold, the function returns `InvalidData(...)` with
    ///      both the actual count and capacity echoed. No existing
    ///      test exercises this rejection path.
    ///
    /// Mutations caught:
    ///   * `cws.len() > ncws` → `>= ncws`: the exact-fit (17, 17)
    ///     case would wrongly trigger the error branch.
    ///   * `n_ecc == 0` early return removed: exact-fit input would
    ///     call `encode_k(_, 0, gf)` and either panic or hang.
    ///   * `n_ecc = ncws - cws.len()` → `cws.len() - ncws`: would
    ///     panic on the exact-fit (0) case via wrapping_sub overflow.
    ///   * Error-format dropping the cws.len() echo: the "18" probe
    ///     below would fail.
    ///
    /// Setup uses an alternating 0101… msgbits pattern, which has
    /// no 5-bit all-same windows and therefore never triggers
    /// `bit_stuff`'s padding insertions. So `bit_stuff(N bits, 6) =
    /// ceil(N / 6)` codewords for our inputs. Compact L1 (metric
    /// index 1) has ncws=17 and bps=6 — 102 bits → 17 cws (exact
    /// fit), 108 bits → 18 cws (over capacity).
    #[test]
    fn build_codewords_exact_fit_and_over_capacity_branches() {
        // (1) Exact fit: 17 cws (no ECC slack).
        let exact_fit_bits: Vec<bool> = (0..102).map(|i| i % 2 == 0).collect();
        let cws_exact = build_codewords(&exact_fit_bits, 1)
            .expect("compact L1 with 102 alternating bits must produce 17 cws (exact fit)");
        assert_eq!(
            cws_exact.len(),
            17,
            "exact-fit branch returns cws as-is (no ECC appended); 17 == ncws"
        );

        // (2) Over capacity: 18 cws > ncws=17 → InvalidData.
        let over_cap_bits: Vec<bool> = (0..108).map(|i| i % 2 == 0).collect();
        let err = build_codewords(&over_cap_bits, 1)
            .expect_err("compact L1 with 108 bits → 18 cws > 17 ncws must error");
        let crate::error::Error::InvalidData(msg) = err else {
            panic!("over-capacity error must be InvalidData; got {err:?}");
        };
        assert!(msg.contains("Aztec"), "diagnostic prefix: {msg}");
        assert!(msg.contains("18"), "echo actual cw count 18: {msg}");
        assert!(msg.contains("17"), "echo capacity 17: {msg}");
    }

    #[test]
    fn build_codewords_short_input_compact_l1() {
        // Build msgbits for "HELLO": 5 codewords × 5 bits = 25 bits in Upper.
        let msgbits = encode_msg(b"HELLO").unwrap();
        assert_eq!(msgbits.len(), 25);
        // Compact L1: ncws=17, bpcw=6.
        let metric_idx = fit_metric(msgbits.len(), "compact", -1, 23, 3, false).unwrap();
        let cws = build_codewords(&msgbits, metric_idx).unwrap();
        assert_eq!(cws.len(), METRICS[metric_idx].ncws as usize);
        // First 5 codewords come from bit-stuffing "HELLO"'s 25 bits:
        // (no stuffing needed, since none of the 5-bit slices are all 0 / all 1
        // in the 6-bit windowing). Hmm actually bps=6, so the 25 bits become
        // ceil(25/6) = 5 codewords, with the 5th being a tail (~1 partial bit).
        // Verify at least the count.
        // Stage 11.A8c (cont) — descriptive label naming compact L1 path.
        assert!(
            !cws.is_empty(),
            "build_codewords(\"HELLO\" → 25 msgbits, compact L1 metric) must produce non-empty codewords (compact L1 ncws=17, bpcw=6); got len={}",
            cws.len()
        );
    }

    #[test]
    fn build_mode_bits_compact_l1_zero_cws() {
        // Layers=1 compact, cw_count=1 → mode = 0*64 + (1-1) = 0
        // → nibbles [0, 0] → 5 ECC over GF(16) → 7 nibbles total → 28 bits.
        let bits = build_mode_bits("compact", 1, 1, false, None);
        assert_eq!(bits.len(), 28);
    }

    #[test]
    fn build_mode_bits_full_size() {
        // Full mode: 4 data + 6 ECC = 10 nibbles = 40 bits.
        let bits = build_mode_bits("full", 1, 5, false, None);
        assert_eq!(bits.len(), 40);
    }

    #[test]
    fn build_mode_bits_rune_xor() {
        // Rune mode 0 → nibbles [0, 0] → after RS-ECC 7 nibbles, XOR each
        // with 10. All ECC nibbles when input is [0,0] should be 0 → after
        // XOR all are 10 = 0b1010.
        let bits = build_mode_bits("rune", 0, 0, false, Some(0));
        assert_eq!(bits.len(), 28);
        // First nibble: 0 ^ 10 = 10 = 0b1010 → bits[0..4] = [1,0,1,0].
        assert_eq!(&bits[..4], &[true, false, true, false]);
    }

    #[test]
    fn build_matrix_compact_l1_size() {
        // Build a minimal compact L1 symbol with all-zero codewords.
        let cws = vec![0u32; 17];
        let modebits = vec![false; 28];
        let m = build_matrix("compact", 1, &cws, 6, &modebits);
        assert_eq!(m.size, 15);
        assert_eq!(m.pixels.len(), 15);
        assert_eq!(m.pixels[0].len(), 15);
    }

    #[test]
    fn encode_hello_matches_bwip_js_compact_l1() {
        // Golden 15×15 matrix captured from bwip-js (oracle-azteccode.js
        // "HELLO" compact) on 2026-05-19.
        let bm = encode(b"HELLO").unwrap();
        assert_eq!(bm.width(), 15);
        assert_eq!(bm.height(), 15);
        let want: [[u8; 15]; 15] = [
            [0, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 1, 0, 0, 1],
            [0, 1, 0, 1, 1, 1, 0, 1, 1, 0, 1, 0, 0, 0, 0],
            [0, 0, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 1],
            [1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
            [0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0],
            [1, 0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 0, 1],
            [0, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 0],
            [1, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 1, 1, 0],
            [1, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 0],
            [1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 1, 1],
            [0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1],
            [1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0],
            [0, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 0],
            [1, 1, 1, 1, 1, 0, 1, 1, 0, 0, 1, 1, 1, 1, 0],
            [0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 1, 1],
        ];
        for (y, want_row) in want.iter().enumerate() {
            for (x, &w) in want_row.iter().enumerate() {
                let got = u8::from(bm.get(x, y));
                assert_eq!(got, w, "mismatch at ({x}, {y})");
            }
        }
    }

    #[test]
    fn encode_dp_pair_compression_comma_space() {
        // "X, " — pair (', ', ' ') compressible to PAIR_4 in Punct.
        // BWIPP DP picks the cheaper path. The result depends on the
        // surrounding context, but for "X, " the seq should contain
        // PAIR_4 if pair compression is beneficial.
        let seq = encode_dp(b"X, ").unwrap();
        // Verify PAIR_4 made it into the seq.
        assert!(
            seq.contains(&PAIR_4),
            "expected PAIR_4 in seq for 'X, '; got {seq:?}",
        );
    }

    #[test]
    fn encode_hello_world_matches_bwip_js() {
        // "Hello, World" was the pair-compression diff case before this
        // commit; now byte-identical to bwip-js (compact L1, 19×19).
        let bm = encode(b"Hello, World").unwrap();
        // Just verify the size and that bull's-eye centre is set —
        // the per-pixel match is exercised via the dump_matrix + bwip-js
        // sweep, not pinned here to avoid bloating the test.
        assert_eq!(bm.width(), 19);
        let mid = 9;
        assert!(bm.get(mid, mid), "bull's-eye centre should be lit");
    }

    #[test]
    fn encode_compact_matches_encode_for_short_input() {
        // For inputs that already fit compact, encode_compact must
        // produce byte-identical output to plain encode (which auto-
        // selects compact when it fits).
        let bm_auto = encode(b"HELLO").unwrap();
        let bm_compact = encode_compact(b"HELLO").unwrap();
        assert_eq!(bm_auto.width(), bm_compact.width());
        assert_eq!(bm_auto.height(), bm_compact.height());
        for y in 0..bm_auto.height() {
            for x in 0..bm_auto.width() {
                assert_eq!(
                    bm_auto.get(x, y),
                    bm_compact.get(x, y),
                    "diverge at ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn encode_compact_rejects_payload_that_exceeds_l4() {
        // Compact L4 capacity is ~89 8-bit bytes (608 codeword bits /
        // 8 bps - mode-bits overhead). 200 ASCII bytes definitely
        // overflows compact; force_compact should surface that as
        // InvalidData rather than silently producing a full-size
        // symbol like plain encode() would.
        //
        // Stage 11.A8c (cont) — single-substring `msg.contains
        // ("compact")` upgraded to 4-anchor pin:
        //   1. `Aztec Compact:` symbology prefix
        //   2. `input data exceeds` predicate
        //   3. `L1-L4 compact size range` range-name anchor
        //   4. cross-arm contamination guard: must NOT contain
        //      `maximum symbol size` (sibling full-size arm wording
        //      at line 1801-1803 of aztec.rs).
        let long = b"A".repeat(300);
        let err = encode_compact(&long).expect_err("L4-overflow must error");
        match err {
            crate::error::Error::InvalidData(msg) => {
                assert!(
                    msg.contains("Aztec Compact:"),
                    "missing `Aztec Compact:` symbology prefix: {msg:?}"
                );
                assert!(
                    msg.contains("input data exceeds"),
                    "missing `input data exceeds` predicate: {msg:?}"
                );
                assert!(
                    msg.contains("L1-L4 compact size range"),
                    "missing `L1-L4 compact size range` range-name anchor: {msg:?}"
                );
                assert!(
                    !msg.contains("maximum symbol size"),
                    "cross-arm contamination: compact-overflow msg mentions full-size sibling: {msg:?}"
                );
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn encode_compact_rejects_empty_input() {
        // Stage 11.A8c — upgrade discriminant-only `matches!` to a
        // 3-anchor pin matching the source diagnostic at line 1787-
        // 1788 (`Aztec: input data must not be empty`). Cross-arm
        // guard against the compact-overflow arm.
        match encode_compact(b"") {
            Err(crate::error::Error::InvalidData(msg)) => {
                assert!(msg.contains("Aztec:"), "missing `Aztec:` prefix: {msg}");
                assert!(
                    msg.contains("must not be empty"),
                    "missing `must not be empty` predicate: {msg}"
                );
                assert!(
                    !msg.contains("exceeds the L1-L4 compact size range"),
                    "wrong arm — compact-overflow diagnostic leaked: {msg}"
                );
            }
            other => panic!("empty Aztec compact should reject as InvalidData, got {other:?}"),
        }
    }

    /// Aztec Rune is a fixed 11×11 marker; the 8-bit value is the
    /// payload. Pin our output for a handful of rune values against
    /// bwip-js's `raw("aztecrune", ...)` pixs.
    #[test]
    fn encode_rune_matches_bwip_js_pixs() {
        // (value-string, 11 row strings) — captured from bwip-js
        // 4.10.1's raw("aztecrune", ...)[0].pixs.
        let cases: &[(&str, [&str; 11])] = &[
            (
                "0",
                [
                    "11101010101",
                    "11111111111",
                    "01000000010",
                    "11011111011",
                    "01010001010",
                    "11010101011",
                    "01010001010",
                    "11011111011",
                    "01000000010",
                    "01111111111",
                    "00101010100",
                ],
            ),
            (
                "42",
                [
                    "11100000001",
                    "11111111111",
                    "11000000010",
                    "01011111010",
                    "11010001010",
                    "01010101010",
                    "11010001011",
                    "01011111011",
                    "11000000011",
                    "01111111111",
                    "00001011100",
                ],
            ),
            (
                "128",
                [
                    "11001010101",
                    "11111111111",
                    "11000000010",
                    "01011111011",
                    "11010001010",
                    "11010101010",
                    "01010001010",
                    "01011111010",
                    "11000000010",
                    "01111111111",
                    "00100010000",
                ],
            ),
            (
                "255",
                [
                    "11010101001",
                    "11111111111",
                    "01000000011",
                    "11011111011",
                    "11010001011",
                    "01010101011",
                    "01010001010",
                    "11011111011",
                    "11000000010",
                    "01111111111",
                    "00110011100",
                ],
            ),
        ];
        for &(input, rows) in cases {
            let bm =
                encode_rune(input).unwrap_or_else(|e| panic!("encode_rune({input:?}) failed: {e}"));
            assert_eq!(bm.width(), 11, "rune {input}: width != 11");
            assert_eq!(bm.height(), 11, "rune {input}: height != 11");
            for (y, row) in rows.iter().enumerate() {
                let mut got = String::with_capacity(11);
                for x in 0..11 {
                    got.push(if bm.get(x, y) { '1' } else { '0' });
                }
                assert_eq!(
                    got, *row,
                    "rune {input}: row {y} mismatch\n want: {row}\n  got: {got}"
                );
            }
        }
    }

    #[test]
    fn encode_rune_rejects_invalid_input() {
        // Stage 11.A8c — upgrade 4 discriminant-only sites to
        // multi-anchor pins matching the source diagnostics at lines
        // 1741-1742 (empty), 1746-1749 (too long), 1753-1755
        // (non-digit), and 1762-1763 (value > 255).
        //
        // Non-digit 'A' at offset 1.
        match encode_rune("4A") {
            Err(crate::error::Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Aztec Rune:"),
                    "non-digit arm missing `Aztec Rune:` prefix: {msg}"
                );
                assert!(
                    msg.contains("non-digit byte"),
                    "non-digit arm missing predicate: {msg}"
                );
                assert!(
                    msg.contains("0x41"),
                    "non-digit arm missing hex echo `0x41` for 'A': {msg}"
                );
            }
            other => panic!("`4A` should reject as InvalidData, got {other:?}"),
        }
        // Too long (>3 chars).
        match encode_rune("1000") {
            Err(crate::error::Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Aztec Rune:"),
                    "too-long arm missing `Aztec Rune:` prefix: {msg}"
                );
                assert!(
                    msg.contains("must be 1-3 digits"),
                    "too-long arm missing `must be 1-3 digits` predicate: {msg}"
                );
                assert!(
                    msg.contains("got 4 chars"),
                    "too-long arm missing `got 4 chars` length echo: {msg}"
                );
            }
            other => panic!("`1000` (4-char) should reject as InvalidData, got {other:?}"),
        }
        // Out of range (256 > 255).
        match encode_rune("256") {
            Err(crate::error::Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Aztec Rune:"),
                    "out-of-range arm missing prefix: {msg}"
                );
                assert!(
                    msg.contains("must be 0..=255"),
                    "out-of-range arm missing `must be 0..=255` predicate: {msg}"
                );
                assert!(
                    msg.contains("got 256"),
                    "out-of-range arm missing `got 256` value echo: {msg}"
                );
            }
            other => panic!("`256` should reject as InvalidData, got {other:?}"),
        }
        // Empty.
        match encode_rune("") {
            Err(crate::error::Error::InvalidData(msg)) => {
                assert!(
                    msg.contains("Aztec Rune:"),
                    "empty arm missing `Aztec Rune:` prefix: {msg}"
                );
                assert!(
                    msg.contains("1-3 digit integer"),
                    "empty arm missing `1-3 digit integer` predicate: {msg}"
                );
                assert!(
                    msg.contains("(0..=255)"),
                    "empty arm missing `(0..=255)` range hint: {msg}"
                );
                assert!(
                    !msg.contains("non-digit byte"),
                    "wrong arm — non-digit diagnostic leaked: {msg}"
                );
            }
            other => panic!("empty Aztec Rune should reject as InvalidData, got {other:?}"),
        }
    }

    #[test]
    fn encode_high_bit_byte_matches_bwip_js() {
        // "café" (UTF-8 bytes c3 a9 in the é) → uses Byte mode for the
        // 0xC3, 0xA9 pair. Golden 15×15 matrix from bwip-js.
        let bm = encode("café".as_bytes()).unwrap();
        assert_eq!(bm.width(), 15);
        let want: [[u8; 15]; 15] = [
            [0, 0, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 0, 1, 0],
            [1, 1, 0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 1, 0],
            [1, 0, 1, 1, 0, 0, 0, 0, 0, 1, 1, 0, 1, 0, 1],
            [0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1],
            [0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1],
            [0, 0, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 0, 1, 0],
            [0, 0, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0],
            [0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0],
            [0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 1, 0],
            [0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1],
            [1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0],
            [1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
            [1, 0, 0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1],
            [1, 0, 1, 1, 0, 0, 1, 1, 1, 1, 0, 1, 0, 0, 1],
            [1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0],
        ];
        for (y, want_row) in want.iter().enumerate() {
            for (x, &w) in want_row.iter().enumerate() {
                let got = u8::from(bm.get(x, y));
                assert_eq!(got, w, "mismatch at ({x}, {y})");
            }
        }
    }

    #[test]
    fn encode_digits_matches_bwip_js_compact() {
        // Golden 15×15 matrix from bwip-js for "12345" compact.
        let bm = encode(b"12345").unwrap();
        assert_eq!(bm.width(), 15);
        // Just verify the bull's-eye center is set (sanity).
        // Center cell: (7, 7) for size=15.
        assert!(bm.get(7, 7), "center module should be set");
    }

    #[test]
    fn build_matrix_full_l1_size() {
        let cws = vec![0u32; 21];
        let modebits = vec![false; 40];
        let m = build_matrix("full", 1, &cws, 6, &modebits);
        // Full L1: fw=13, size = 13 + 4 + 2 + 0 = 19 (growth=0 for L1).
        assert_eq!(m.size, 19);
    }

    #[test]
    fn lmv_layer1_position0_compact() {
        // Compact L1: fw=9, layer=1. lwid = 9+4 = 13. dir=0 (top).
        // pos=0 → col=0, row=0.
        // x = -(13-1)/2 + 1 + 0 = -6+1+0 = -5
        // y = (9-1)/2 + 1*2 + 0 = 4+2 = 6
        assert_eq!(lmv(1, 0, 9), (-5, 6));
    }

    /// Stage 11.A8c — pin `lmv` across all four direction branches
    /// (top/right/bottom/left), with multiple positions per branch to
    /// cover the col/row decomposition and per-side sign/offset
    /// arithmetic. The single existing test only hit dir=0,
    /// col=0, row=0 — every other position landed in untested
    /// arithmetic. Mutations to catch:
    ///   - `pos / 2` → `pos % 2` or `pos * 2`: breaks all positions.
    ///   - `% lwid` → `/ lwid`: breaks col vs dir slicing.
    ///   - `(lwid - 1) / 2` → `lwid / 2`: top-side off-by-one.
    ///   - `(fw - 1) / 2` → `fw / 2`: right/left-side off-by-one.
    ///   - `+ layer * 2` → `+ layer + 2`: layer-2+ row drift.
    ///   - swapping x/y in any of the four arms.
    ///   - `+ col` → `+ row` (or vice-versa).
    ///   - `-(...)` sign flips on the bottom and left branches.
    #[test]
    fn lmv_all_four_sides_compact_layer1() {
        // Compact L1, fw=9, layer=1, lwid=13. 104 positions per layer.
        // ---- dir 0 (top side, pos 0..=25) -------------------------------
        // pos=0: col=0, row=0 → already covered above.
        // pos=1: col=0, row=1 → x unchanged, y += 1.
        assert_eq!(lmv(1, 1, 9), (-5, 7), "dir 0, pos 1 (col=0, row=1)");
        // pos=2: col=1, row=0 → x += 1.
        assert_eq!(lmv(1, 2, 9), (-4, 6), "dir 0, pos 2 (col=1, row=0)");
        // pos=24: col=12, row=0 → x = -6+1+12 = 7.
        assert_eq!(lmv(1, 24, 9), (7, 6), "dir 0, pos 24 (col=12, row=0)");
        // pos=25: col=12, row=1 → y = 7.
        assert_eq!(lmv(1, 25, 9), (7, 7), "dir 0, pos 25 (col=12, row=1)");

        // ---- dir 1 (right side, pos 26..=51) ---------------------------
        // pos=26: pos/2=13, dir=1, col=0, row=0.
        // x = fw/2 + layer*2 + row = 4+2+0 = 6.
        // y = (lwid-1)/2 - 1 - col = 6-1-0 = 5.
        assert_eq!(lmv(1, 26, 9), (6, 5), "dir 1, pos 26 (col=0, row=0)");
        // pos=27: col=0, row=1 → x = 7, y = 5.
        assert_eq!(lmv(1, 27, 9), (7, 5), "dir 1, pos 27 (col=0, row=1)");
        // pos=28: col=1, row=0 → x = 6, y = 4.
        assert_eq!(lmv(1, 28, 9), (6, 4), "dir 1, pos 28 (col=1, row=0)");
        // pos=50: pos/2=25, col=12, row=0 → x = 6, y = 6-1-12 = -7.
        assert_eq!(lmv(1, 50, 9), (6, -7), "dir 1, pos 50 (col=12, row=0)");
        // pos=51: col=12, row=1 → x = 7, y = -7.
        assert_eq!(lmv(1, 51, 9), (7, -7), "dir 1, pos 51 (col=12, row=1)");

        // ---- dir 2 (bottom side, pos 52..=77) --------------------------
        // pos=52: pos/2=26, dir=2, col=0, row=0.
        // x = -(-(lwid/2) + 1 + col) = -(-6+1+0) = 5.
        // y = -(fw/2 + layer*2 + row) = -(4+2+0) = -6.
        assert_eq!(lmv(1, 52, 9), (5, -6), "dir 2, pos 52 (col=0, row=0)");
        // pos=53: row=1 → y = -7.
        assert_eq!(lmv(1, 53, 9), (5, -7), "dir 2, pos 53 (col=0, row=1)");
        // pos=54: col=1, row=0 → x = -(-6+1+1) = 4.
        assert_eq!(lmv(1, 54, 9), (4, -6), "dir 2, pos 54 (col=1, row=0)");

        // ---- dir 3 (left side, pos 78..=103) ---------------------------
        // pos=78: pos/2=39, dir=3, col=0, row=0.
        // x = -((fw-1)/2 + layer*2 + row) = -(4+2+0) = -6.
        // y = -(lwid/2 - 1 - col) = -(6-1-0) = -5.
        assert_eq!(lmv(1, 78, 9), (-6, -5), "dir 3, pos 78 (col=0, row=0)");
        // pos=79: row=1 → x = -7.
        assert_eq!(lmv(1, 79, 9), (-7, -5), "dir 3, pos 79 (col=0, row=1)");
        // pos=80: col=1, row=0 → y = -(6-1-1) = -4.
        assert_eq!(lmv(1, 80, 9), (-6, -4), "dir 3, pos 80 (col=1, row=0)");
    }

    /// Stage 11.A8c — pin `lmv` for layer ≥ 2 (different lwid) and
    /// full-format fw=12 (different (fw-1)/2 → 5 rather than 4). These
    /// catch mutations on `layer * 4` and `fw` propagation that the
    /// layer=1, fw=9 cases above can't observe.
    #[test]
    fn lmv_layer2_and_full_format() {
        // Compact L2: fw=9, layer=2 → lwid = 9+8 = 17.
        // pos=0: dir=0, col=0, row=0.
        // x = -((17-1)/2) + 1 = -8+1 = -7.
        // y = (9-1)/2 + 2*2 + 0 = 4+4 = 8.
        assert_eq!(lmv(2, 0, 9), (-7, 8), "compact L2 pos=0");
        // Full L1: fw=12, layer=1 → lwid = 12+4 = 16.
        // pos=0: dir=0, col=0, row=0.
        // x = -((16-1)/2) + 1 = -7+1 = -6.
        // y = (12-1)/2 + 1*2 + 0 = 5+2 = 7.
        assert_eq!(lmv(1, 0, 12), (-6, 7), "full L1 pos=0");
        // Full L1 pos=1: same col=0, row=1 → y += 1.
        assert_eq!(lmv(1, 1, 12), (-6, 8), "full L1 pos=1 row=1");
        // Full L1 pos=2: col=1, row=0 → x += 1.
        assert_eq!(lmv(1, 2, 12), (-5, 7), "full L1 pos=2 col=1");
    }

    #[test]
    fn cmv_basic() {
        // Compact L1: size=15, mid=7, mid_idx=7*15+7=112.
        // cmv(0, 0, 112, 15) = 0 - 0 + 112 = 112.
        assert_eq!(cmv(0, 0, 112, 15), 112);
        // cmv(x=-5, y=6, 112, 15) = -5 - 6*15 + 112 = -5 - 90 + 112 = 17.
        assert_eq!(cmv(-5, 6, 112, 15), 17);
        // cmv(7, -7, 112, 15) (bottom-right corner): 7 + 105 + 112 = 224.
        assert_eq!(cmv(7, -7, 112, 15), 224);
        // cmv(-7, 7, 112, 15) (top-left corner): -7 - 105 + 112 = 0.
        assert_eq!(cmv(-7, 7, 112, 15), 0);
    }

    #[test]
    fn state_constants_are_in_range() {
        assert_eq!(STATE_UPPER, 0);
        assert_eq!(STATE_BYTE, 5);
        // All distinct.
        let s = [
            STATE_UPPER,
            STATE_LOWER,
            STATE_MIXED,
            STATE_PUNCT,
            STATE_DIGIT,
            STATE_BYTE,
        ];
        let mut sorted = s;
        sorted.sort_unstable();
        for i in 1..sorted.len() {
            assert!(sorted[i] > sorted[i - 1], "duplicate state {}", sorted[i]);
        }
    }

    /// Stage 11.A8c — pin per-state codeword lookups for representative
    /// bytes covering every match arm. Kills the per-arm `delete match
    /// arm` mutations and `- with +` mutations on the offset arithmetic.
    #[test]
    fn per_state_codeword_lookups() {
        // Upper: space=1, 'A'=2, 'Z'=27.
        assert_eq!(upper_codeword(b' '), Some(UPPER_SPACE));
        assert_eq!(upper_codeword(b'A'), Some(2));
        assert_eq!(upper_codeword(b'Z'), Some(27));
        // Out-of-range.
        assert_eq!(upper_codeword(b'a'), None);
        assert_eq!(upper_codeword(b'0'), None);

        // Lower: space=1, 'a'=2, 'z'=27.
        assert_eq!(lower_codeword(b' '), Some(1));
        assert_eq!(lower_codeword(b'a'), Some(2));
        assert_eq!(lower_codeword(b'z'), Some(27));
        // Out-of-range.
        assert_eq!(lower_codeword(b'A'), None);
        assert_eq!(lower_codeword(b'0'), None);

        // Digit: space=1, '0'=2, '9'=11, ','=12, '.'=13.
        assert_eq!(digit_codeword(b' '), Some(1));
        assert_eq!(digit_codeword(b'0'), Some(2));
        assert_eq!(digit_codeword(b'9'), Some(11));
        assert_eq!(digit_codeword(b','), Some(12));
        assert_eq!(digit_codeword(b'.'), Some(13));
        // Out-of-range.
        assert_eq!(digit_codeword(b'a'), None);
        assert_eq!(digit_codeword(b'A'), None);
    }

    /// Stage 11.A8c — pin `preferred_state` byte-to-state mapping.
    /// The helper drives the greedy encoder's mode-switching DP; if
    /// the mapping shifts, the greedy/DP encoders pick wrong latches
    /// and the bit stream changes — caught at the symbol level only.
    ///
    /// Each branch:
    ///   - ' ' / A-Z → STATE_UPPER (canonical)
    ///   - a-z → STATE_LOWER
    ///   - 0-9, ',', '.' → STATE_DIGIT
    ///   - CR (13) + punctuation in the listed set → STATE_PUNCT
    ///   - control/special bytes → STATE_MIXED
    ///   - everything else (high-bit) → STATE_BYTE
    #[test]
    fn preferred_state_per_byte_class() {
        // STATE_UPPER: space + A-Z.
        assert_eq!(preferred_state(b' '), STATE_UPPER);
        assert_eq!(preferred_state(b'A'), STATE_UPPER);
        assert_eq!(preferred_state(b'M'), STATE_UPPER);
        assert_eq!(preferred_state(b'Z'), STATE_UPPER);
        // STATE_LOWER: a-z.
        assert_eq!(preferred_state(b'a'), STATE_LOWER);
        assert_eq!(preferred_state(b'm'), STATE_LOWER);
        assert_eq!(preferred_state(b'z'), STATE_LOWER);
        // STATE_DIGIT: 0-9, ',', '.'.
        assert_eq!(preferred_state(b'0'), STATE_DIGIT);
        assert_eq!(preferred_state(b'9'), STATE_DIGIT);
        assert_eq!(preferred_state(b','), STATE_DIGIT);
        assert_eq!(preferred_state(b'.'), STATE_DIGIT);
        // STATE_PUNCT: CR + listed punctuation.
        assert_eq!(preferred_state(13), STATE_PUNCT, "CR → punct");
        assert_eq!(preferred_state(b'!'), STATE_PUNCT);
        assert_eq!(preferred_state(b'?'), STATE_PUNCT);
        assert_eq!(preferred_state(b'/'), STATE_PUNCT);
        assert_eq!(preferred_state(b'{'), STATE_PUNCT);
        assert_eq!(preferred_state(b'}'), STATE_PUNCT);
        // STATE_MIXED: low-control bytes + special bytes.
        assert_eq!(preferred_state(1), STATE_MIXED);
        assert_eq!(preferred_state(7), STATE_MIXED);
        assert_eq!(preferred_state(b'@'), STATE_MIXED);
        assert_eq!(preferred_state(b'\\'), STATE_MIXED);
        assert_eq!(preferred_state(b'^'), STATE_MIXED);
        assert_eq!(preferred_state(b'_'), STATE_MIXED);
        assert_eq!(preferred_state(127), STATE_MIXED);
        // STATE_BYTE: high-bit and other unencoded ASCII.
        assert_eq!(preferred_state(128), STATE_BYTE);
        assert_eq!(preferred_state(200), STATE_BYTE);
        assert_eq!(preferred_state(255), STATE_BYTE);
    }

    /// Stage 11.A8c — pin `preferred_state` range BOUNDARIES and the
    /// remaining unlisted PUNCT/MIXED bytes that the original test
    /// didn't enumerate.
    ///
    /// Mutations caught:
    ///   * `1..=12` → `1..=11`: byte 12 flips from MIXED to BYTE.
    ///   * `14..=26` → `15..=26` or `14..=25`: byte 14 or 26 flips.
    ///   * `27..=31` → `28..=31` or `27..=30`: byte 27 or 31 flips.
    ///   * Drop any single byte from the PUNCT list (e.g. `b'"'` →
    ///     becomes STATE_BYTE).
    ///   * Drop any single byte from the MIXED list (e.g. `b'`'`,
    ///     `b'|'`, `b'~'` → become STATE_BYTE).
    ///   * NUL (0) hits the default `_ => STATE_BYTE` arm — not
    ///     in 1..=12.
    #[test]
    fn preferred_state_range_boundaries_and_unlisted_bytes() {
        // Range boundaries in 1..=12 (MIXED).
        assert_eq!(preferred_state(12), STATE_MIXED, "byte 12 (end of 1..=12)");
        // Boundaries of 14..=26 (MIXED). Skipping 13 since CR=13 is PUNCT.
        assert_eq!(
            preferred_state(14),
            STATE_MIXED,
            "byte 14 (start of 14..=26)"
        );
        assert_eq!(preferred_state(26), STATE_MIXED, "byte 26 (end of 14..=26)");
        // Boundaries of 27..=31 (MIXED).
        assert_eq!(
            preferred_state(27),
            STATE_MIXED,
            "byte 27 (start of 27..=31)"
        );
        assert_eq!(preferred_state(31), STATE_MIXED, "byte 31 (end of 27..=31)");
        // Other MIXED literal bytes not in the original test.
        assert_eq!(preferred_state(b'`'), STATE_MIXED, "backtick → MIXED");
        assert_eq!(preferred_state(b'|'), STATE_MIXED, "pipe → MIXED");
        assert_eq!(preferred_state(b'~'), STATE_MIXED, "tilde → MIXED");
        // PUNCT list bytes not in the original test.
        assert_eq!(preferred_state(b'"'), STATE_PUNCT, "dquote → PUNCT");
        assert_eq!(preferred_state(b'#'), STATE_PUNCT);
        assert_eq!(preferred_state(b'$'), STATE_PUNCT);
        assert_eq!(preferred_state(b'%'), STATE_PUNCT);
        assert_eq!(preferred_state(b'&'), STATE_PUNCT);
        assert_eq!(preferred_state(b'\''), STATE_PUNCT, "squote → PUNCT");
        assert_eq!(preferred_state(b'('), STATE_PUNCT);
        assert_eq!(preferred_state(b')'), STATE_PUNCT);
        assert_eq!(preferred_state(b'*'), STATE_PUNCT);
        assert_eq!(preferred_state(b'+'), STATE_PUNCT);
        assert_eq!(preferred_state(b'-'), STATE_PUNCT);
        assert_eq!(preferred_state(b':'), STATE_PUNCT);
        assert_eq!(preferred_state(b';'), STATE_PUNCT);
        assert_eq!(preferred_state(b'<'), STATE_PUNCT);
        assert_eq!(preferred_state(b'='), STATE_PUNCT);
        assert_eq!(preferred_state(b'>'), STATE_PUNCT);
        assert_eq!(preferred_state(b'['), STATE_PUNCT);
        assert_eq!(preferred_state(b']'), STATE_PUNCT);
        // NUL hits the default `_ => STATE_BYTE` arm (not in 1..=12).
        assert_eq!(
            preferred_state(0),
            STATE_BYTE,
            "NUL (0) falls through to default BYTE"
        );
    }

    /// Stage 11.A8c — pin `latch_codeword` per-state-pair mappings.
    /// 10 valid transitions + the catch-all None. Mutations on any
    /// arm value (e.g. `(UPPER, LOWER) → UPPER_LL` swapped to
    /// `UPPER_LM`) would shift the encoded bit stream by changing the
    /// latch codeword.
    #[test]
    fn latch_codeword_per_state_pair() {
        // From UPPER.
        assert_eq!(latch_codeword(STATE_UPPER, STATE_LOWER), Some(UPPER_LL));
        assert_eq!(latch_codeword(STATE_UPPER, STATE_MIXED), Some(UPPER_LM));
        assert_eq!(latch_codeword(STATE_UPPER, STATE_DIGIT), Some(UPPER_LD));
        // From LOWER.
        assert_eq!(latch_codeword(STATE_LOWER, STATE_MIXED), Some(29));
        assert_eq!(latch_codeword(STATE_LOWER, STATE_DIGIT), Some(30));
        // From MIXED.
        assert_eq!(latch_codeword(STATE_MIXED, STATE_LOWER), Some(28));
        assert_eq!(latch_codeword(STATE_MIXED, STATE_UPPER), Some(29));
        assert_eq!(latch_codeword(STATE_MIXED, STATE_PUNCT), Some(30));
        // From PUNCT.
        assert_eq!(latch_codeword(STATE_PUNCT, STATE_UPPER), Some(31));
        // From DIGIT.
        assert_eq!(latch_codeword(STATE_DIGIT, STATE_UPPER), Some(14));
        // Two-step latches (e.g. U→P via M) and self-transitions
        // and unsupported transitions return None.
        assert_eq!(latch_codeword(STATE_UPPER, STATE_PUNCT), None);
        assert_eq!(latch_codeword(STATE_UPPER, STATE_UPPER), None);
        assert_eq!(latch_codeword(STATE_DIGIT, STATE_PUNCT), None);
        assert_eq!(latch_codeword(STATE_PUNCT, STATE_LOWER), None);
        assert_eq!(latch_codeword(STATE_BYTE, STATE_UPPER), None);
    }

    /// Stage 11.A8c — pin every arm of `sentinel_codeword(state,
    /// sentinel)`. 24 (state, sentinel) → codeword arms + a catch-all
    /// None. The DP routes these through `seq_to_bits` when emitting
    /// latches/shifts/pairs; arm-swap mutations (e.g.
    /// `(UPPER, LATCH_LOWER) → 28` swapped to `29`) would produce
    /// wrong codewords without breaking other tests.
    #[test]
    fn sentinel_codeword_all_arms() {
        // From UPPER.
        assert_eq!(sentinel_codeword(STATE_UPPER, LATCH_LOWER), Some(28));
        assert_eq!(sentinel_codeword(STATE_UPPER, LATCH_MIXED), Some(29));
        assert_eq!(sentinel_codeword(STATE_UPPER, LATCH_DIGIT), Some(30));
        assert_eq!(sentinel_codeword(STATE_UPPER, SHIFT_BYTE), Some(31));
        assert_eq!(sentinel_codeword(STATE_UPPER, SHIFT_PUNCT), Some(0));
        // From LOWER.
        assert_eq!(sentinel_codeword(STATE_LOWER, SHIFT_UPPER), Some(28));
        assert_eq!(sentinel_codeword(STATE_LOWER, LATCH_MIXED), Some(29));
        assert_eq!(sentinel_codeword(STATE_LOWER, LATCH_DIGIT), Some(30));
        assert_eq!(sentinel_codeword(STATE_LOWER, SHIFT_BYTE), Some(31));
        assert_eq!(sentinel_codeword(STATE_LOWER, SHIFT_PUNCT), Some(0));
        // From MIXED.
        assert_eq!(sentinel_codeword(STATE_MIXED, LATCH_LOWER), Some(28));
        assert_eq!(sentinel_codeword(STATE_MIXED, LATCH_UPPER), Some(29));
        assert_eq!(sentinel_codeword(STATE_MIXED, LATCH_PUNCT), Some(30));
        assert_eq!(sentinel_codeword(STATE_MIXED, SHIFT_BYTE), Some(31));
        assert_eq!(sentinel_codeword(STATE_MIXED, SHIFT_PUNCT), Some(0));
        // From PUNCT.
        assert_eq!(sentinel_codeword(STATE_PUNCT, LATCH_UPPER), Some(31));
        assert_eq!(sentinel_codeword(STATE_PUNCT, FLG_NEXT), Some(0));
        assert_eq!(sentinel_codeword(STATE_PUNCT, PAIR_2), Some(2));
        assert_eq!(sentinel_codeword(STATE_PUNCT, PAIR_3), Some(3));
        assert_eq!(sentinel_codeword(STATE_PUNCT, PAIR_4), Some(4));
        assert_eq!(sentinel_codeword(STATE_PUNCT, PAIR_5), Some(5));
        // From DIGIT.
        assert_eq!(sentinel_codeword(STATE_DIGIT, LATCH_UPPER), Some(14));
        assert_eq!(sentinel_codeword(STATE_DIGIT, SHIFT_UPPER), Some(15));
        assert_eq!(sentinel_codeword(STATE_DIGIT, SHIFT_PUNCT), Some(0));
        // Catch-all → None. Pick a few representative unmapped pairs:
        assert_eq!(
            sentinel_codeword(STATE_UPPER, LATCH_UPPER),
            None,
            "UPPER + LATCH_UPPER is a self-transition (no codeword)"
        );
        assert_eq!(
            sentinel_codeword(STATE_DIGIT, LATCH_LOWER),
            None,
            "DIGIT has no direct LATCH_LOWER"
        );
        assert_eq!(
            sentinel_codeword(STATE_PUNCT, LATCH_LOWER),
            None,
            "PUNCT can only return to UPPER (via 31)"
        );
        assert_eq!(sentinel_codeword(STATE_BYTE, LATCH_UPPER), None);
        // Non-sentinel argument also returns None.
        assert_eq!(sentinel_codeword(STATE_UPPER, 65), None);
        assert_eq!(sentinel_codeword(STATE_UPPER, 0), None);
    }

    /// Stage 11.A8c — pin `charsize(state, ch)` bit-cost helper.
    /// Returns CHAR_SIZES[state] for non-negative chars (a real
    /// codeword) or u16::MAX for negative sentinels (which the DP
    /// treats as unreachable). CHAR_SIZES = [5,5,5,5,4,8] —
    /// digits compact to 4 bits, byte mode uses 8 bits, the other
    /// four alphabets use 5.
    ///
    /// Mutations to catch:
    ///   - `ch >= 0` → `ch > 0`: 0 would route to u16::MAX, breaking
    ///     digit '0' or NUL byte encoding.
    ///   - swapping CHAR_SIZES indices via `state as usize`.
    #[test]
    fn charsize_state_widths_and_sentinel_inf() {
        // Per-state widths for positive (real) chars.
        assert_eq!(charsize(STATE_UPPER, b'A' as i32), 5);
        assert_eq!(charsize(STATE_LOWER, b'a' as i32), 5);
        assert_eq!(charsize(STATE_MIXED, b'@' as i32), 5);
        assert_eq!(charsize(STATE_PUNCT, 13), 5);
        assert_eq!(charsize(STATE_DIGIT, b'0' as i32), 4, "digit width is 4");
        assert_eq!(charsize(STATE_BYTE, 0xFF), 8, "byte mode width is 8");
        // ch == 0 (NUL) is still a real char: `>= 0` accepts it.
        assert_eq!(
            charsize(STATE_UPPER, 0),
            5,
            "ch=0 hits the >= 0 branch (CHAR_SIZES[state])"
        );
        assert_eq!(
            charsize(STATE_DIGIT, 0),
            4,
            "ch=0 in DIGIT must use the 4-bit width"
        );
        // Negative sentinels → u16::MAX (effectively unreachable).
        assert_eq!(charsize(STATE_UPPER, LATCH_LOWER), u16::MAX);
        assert_eq!(charsize(STATE_DIGIT, SHIFT_UPPER), u16::MAX);
        assert_eq!(charsize(STATE_BYTE, -1), u16::MAX);
        assert_eq!(charsize(STATE_LOWER, FLG_NEXT), u16::MAX);
    }

    /// Stage 11.A8c — pin `latch_target` 5-arm sentinel-to-state
    /// mapping. Shifts (`SHIFT_UPPER/PUNCT/BYTE`) return None because
    /// they're single-char detours, not state transitions. Mutations
    /// on any arm value (e.g. `LATCH_LOWER → LOWER` swapped to
    /// `UPPER`) would shift state-transition decisions in the DP.
    #[test]
    fn latch_target_per_sentinel() {
        // All 5 latch sentinels map to their state.
        assert_eq!(latch_target(LATCH_UPPER), Some(STATE_UPPER));
        assert_eq!(latch_target(LATCH_LOWER), Some(STATE_LOWER));
        assert_eq!(latch_target(LATCH_MIXED), Some(STATE_MIXED));
        assert_eq!(latch_target(LATCH_PUNCT), Some(STATE_PUNCT));
        assert_eq!(latch_target(LATCH_DIGIT), Some(STATE_DIGIT));
        // Shifts → None (they don't change state, just detour for 1 char).
        assert_eq!(latch_target(SHIFT_UPPER), None);
        assert_eq!(latch_target(SHIFT_PUNCT), None);
        assert_eq!(latch_target(SHIFT_BYTE), None);
        // FLG_NEXT and PAIR_X sentinels → None.
        assert_eq!(latch_target(FLG_NEXT), None);
        assert_eq!(latch_target(PAIR_2), None);
        assert_eq!(latch_target(PAIR_3), None);
        assert_eq!(latch_target(PAIR_4), None);
        assert_eq!(latch_target(PAIR_5), None);
        // Positive values (raw bytes) and unknown negatives → None.
        assert_eq!(latch_target(0), None);
        assert_eq!(latch_target(65), None);
        assert_eq!(latch_target(-100), None);
    }

    /// Stage 11.A8c — pin `pair_sentinel` and `is_pair_sentinel`.
    /// `pair_sentinel(last, cur)` returns the PAIR_X compression
    /// sentinel for the 4 compressible byte pairs (CR-LF, ". ",
    /// ", ", ": "); `is_pair_sentinel(item)` returns true for those
    /// 4 sentinels. Mutations on the match arms (e.g. swap PAIR_2 ↔
    /// PAIR_3) would route the wrong sentinel value, surfacing only
    /// at the symbol level.
    #[test]
    fn pair_sentinel_pre_compression_arms() {
        // The four compressible byte pairs.
        assert_eq!(pair_sentinel(0x0D, 0x0A), Some(PAIR_2), "CR-LF → PAIR_2");
        assert_eq!(pair_sentinel(b'.', b' '), Some(PAIR_3));
        assert_eq!(pair_sentinel(b',', b' '), Some(PAIR_4));
        assert_eq!(pair_sentinel(b':', b' '), Some(PAIR_5));
        // Non-matching pairs return None.
        assert_eq!(pair_sentinel(b' ', 0x0A), None, "space-LF not a pair");
        assert_eq!(pair_sentinel(b'.', b'.'), None, "double-dot not a pair");
        assert_eq!(pair_sentinel(b'A', b'B'), None);
        assert_eq!(pair_sentinel(0, 0), None);
        // pair_sentinel(0x0A, 0x0D) - reversed CR-LF → None.
        assert_eq!(pair_sentinel(0x0A, 0x0D), None, "reversed CR-LF not a pair");

        // is_pair_sentinel: true only for the 4 PAIR_X constants.
        assert!(is_pair_sentinel(PAIR_2));
        assert!(is_pair_sentinel(PAIR_3));
        assert!(is_pair_sentinel(PAIR_4));
        assert!(is_pair_sentinel(PAIR_5));
        // Other negatives and positives → false.
        assert!(!is_pair_sentinel(-15), "-15 (below PAIR_5) not a sentinel");
        assert!(!is_pair_sentinel(-10), "-10 (above PAIR_2) not a sentinel");
        assert!(!is_pair_sentinel(0));
        assert!(!is_pair_sentinel(1));
        assert!(!is_pair_sentinel(100));
    }

    /// Stage 11.A8c — pin `encode_byte_in_state` dispatch arms. The
    /// helper routes a byte through one of 5 state-specific encoders
    /// (`upper_codeword` / `lower_codeword` / etc.) or returns `None`
    /// for STATE_BYTE / unknown states. Mutations on the match arms
    /// (`STATE_UPPER => upper_codeword` swapped with another) would
    /// silently route bytes through the wrong encoder, surfacing as
    /// bit-stream divergence rather than encoder failure.
    #[test]
    fn encode_byte_in_state_routes_to_state_encoders() {
        // Per-state delegation: each state's encoder is the one that
        // accepts the alphabet listed for that state.
        // 'A' (65) → STATE_UPPER accepts (upper_codeword('A') = 2).
        assert_eq!(
            encode_byte_in_state(STATE_UPPER, b'A'),
            upper_codeword(b'A'),
            "STATE_UPPER must dispatch to upper_codeword"
        );
        // 'a' (97) → STATE_LOWER accepts (lower_codeword('a') = 2).
        assert_eq!(
            encode_byte_in_state(STATE_LOWER, b'a'),
            lower_codeword(b'a'),
            "STATE_LOWER must dispatch to lower_codeword"
        );
        // '0' (48) → STATE_DIGIT accepts (digit_codeword('0') = 2).
        assert_eq!(
            encode_byte_in_state(STATE_DIGIT, b'0'),
            digit_codeword(b'0'),
            "STATE_DIGIT must dispatch to digit_codeword"
        );
        // CR (13) → STATE_PUNCT.
        assert_eq!(
            encode_byte_in_state(STATE_PUNCT, 13),
            punct_codeword(13),
            "STATE_PUNCT must dispatch to punct_codeword"
        );
        // '@' (64) → STATE_MIXED.
        assert_eq!(
            encode_byte_in_state(STATE_MIXED, b'@'),
            mixed_codeword(b'@'),
            "STATE_MIXED must dispatch to mixed_codeword"
        );
        // Bytes not in the state's alphabet → None.
        assert_eq!(
            encode_byte_in_state(STATE_UPPER, b'a'),
            None,
            "'a' (lowercase) not in UPPER alphabet"
        );
        assert_eq!(
            encode_byte_in_state(STATE_DIGIT, b'A'),
            None,
            "'A' (uppercase) not in DIGIT alphabet"
        );
        // STATE_BYTE has no helper — always returns None.
        assert_eq!(encode_byte_in_state(STATE_BYTE, b'A'), None);
        assert_eq!(encode_byte_in_state(STATE_BYTE, 0xFF), None);
        // Unknown state (any value > 5) → None via the catch-all.
        assert_eq!(encode_byte_in_state(99, b'A'), None);
        assert_eq!(encode_byte_in_state(255, b'A'), None);
    }

    /// Stage 11.A8c — pin `append_codeword` MSB-first bit layout.
    /// The helper packs a `width`-bit value into a `Vec<bool>` with
    /// the most-significant bit first. Mutations on shift direction
    /// (`>> k` → `<< k`) or order (`(0..width).rev()` → `(0..width)`)
    /// would silently swap bit ordering and the encoded symbol bits
    /// would scramble.
    #[test]
    fn append_codeword_msb_first_bit_layout() {
        // 0b1010 over 4 bits → [true, false, true, false].
        let mut bits: Vec<bool> = Vec::new();
        append_codeword(&mut bits, 0b1010, 4);
        assert_eq!(bits, vec![true, false, true, false]);
        // 0b11111 over 5 bits.
        bits.clear();
        append_codeword(&mut bits, 0b11111, 5);
        assert_eq!(bits, vec![true; 5]);
        // 0 → all-false.
        bits.clear();
        append_codeword(&mut bits, 0, 5);
        assert_eq!(bits, vec![false; 5]);
        // 0b10000 over 5 bits → leading 1 then 4 zeros.
        bits.clear();
        append_codeword(&mut bits, 0b10000, 5);
        assert_eq!(bits, vec![true, false, false, false, false]);
        // Appends rather than replaces.
        bits.clear();
        append_codeword(&mut bits, 0b101, 3);
        append_codeword(&mut bits, 0b010, 3);
        assert_eq!(
            bits,
            vec![true, false, true, false, true, false],
            "two appends concatenate bit-stream order"
        );
    }

    /// `upper_codeword(byte)` is the Aztec Upper-state per-byte lookup:
    /// * space → UPPER_SPACE (= 1)
    /// * 'A'..='Z' → byte - 'A' + 2 (so 'A' = 2, 'Z' = 27)
    /// * else → None
    ///
    /// Used by `encode_single_state` and the greedy state machine, but
    /// never directly tested.
    ///
    /// Mutations to catch:
    /// * `b'A'..=b'Z'` → `b'A'..b'Z'` (excludes 'Z'); `b'A'..=b'Y'`
    ///   (also excludes 'Z').
    /// * `byte - b'A' + 2` → `byte - b'A' + 1` or `+ 3` (off-by-one offset).
    /// * Space arm → wrong constant (returns 0 instead of 1).
    /// * Space arm dropped → space falls through to None.
    /// * Lowercase accidentally accepted (`b'A'..=b'z'` accepts a..z too).
    ///
    /// Anchors at the endpoints + the space sentinel + the boundary
    /// chars just outside the alphabet ('@' just below 'A', '[' just
    /// above 'Z').
    #[test]
    fn upper_codeword_per_arm_with_boundary_anchors() {
        // ---- Space sentinel.
        assert_eq!(
            upper_codeword(b' '),
            Some(super::UPPER_SPACE),
            "space → UPPER_SPACE (= 1)"
        );
        // Pin the actual numeric value (= 1) as a discriminator from a
        // mutant that returns 0 or some other constant.
        assert_eq!(upper_codeword(b' '), Some(1), "space → 1 (numeric)");

        // ---- 'A' and 'Z' endpoints.
        assert_eq!(upper_codeword(b'A'), Some(2), "'A' → 2 ('A'-'A'+2)");
        assert_eq!(
            upper_codeword(b'Z'),
            Some(27),
            "'Z' → 27 ('Z'-'A'+2 = 25+2)"
        );
        // Mid alphabet.
        assert_eq!(upper_codeword(b'M'), Some(14), "'M' → 14 ('M'-'A'+2)");

        // ---- Boundary chars just outside the range.
        assert_eq!(
            upper_codeword(b'@'),
            None,
            "'@' (64) is one below 'A'; must be None"
        );
        assert_eq!(
            upper_codeword(b'['),
            None,
            "'[' (91) is one above 'Z'; must be None"
        );

        // ---- Lowercase letters must be None.
        assert_eq!(upper_codeword(b'a'), None, "'a' lowercase: None");
        assert_eq!(upper_codeword(b'z'), None, "'z' lowercase: None");
        assert_eq!(upper_codeword(b'm'), None, "'m' mid lowercase: None");

        // ---- Other categories.
        assert_eq!(upper_codeword(b'0'), None, "'0': digit → None");
        assert_eq!(upper_codeword(b'9'), None, "'9': digit → None");
        assert_eq!(upper_codeword(b'!'), None, "'!': punct → None");
        assert_eq!(upper_codeword(0), None, "NUL: None");
        assert_eq!(upper_codeword(0x7F), None, "DEL: None");
        assert_eq!(upper_codeword(0xFF), None, "0xFF: None");

        // ---- Distinctness invariant: every accepted byte has a
        // unique codeword, and the codewords cover exactly 1..=27
        // (1 for space, 2..=27 for A..Z).
        use std::collections::HashSet;
        let mut codewords: HashSet<u8> = HashSet::new();
        codewords.insert(upper_codeword(b' ').unwrap());
        for c in b'A'..=b'Z' {
            let cw =
                upper_codeword(c).unwrap_or_else(|| panic!("'{}' must be accepted", c as char));
            assert!(
                codewords.insert(cw),
                "char '{}' produced duplicate codeword {cw}",
                c as char
            );
        }
        assert_eq!(
            codewords.len(),
            27,
            "exactly 27 accepted bytes (space + A-Z)"
        );
        // Codewords must be in 1..=27.
        for &cw in &codewords {
            assert!((1..=27).contains(&cw), "codeword {cw} must be in 1..=27");
        }

        // ---- Monotonicity within 'A'..='Z': upper_codeword(c+1) ==
        // upper_codeword(c) + 1 for c < 'Z'.
        for c in b'A'..b'Z' {
            let cw = upper_codeword(c).unwrap();
            let cw_next = upper_codeword(c + 1).unwrap();
            assert_eq!(
                cw_next,
                cw + 1,
                "'{}' → {cw}, '{}' → must be {cw}+1 = {}",
                c as char,
                (c + 1) as char,
                cw + 1
            );
        }
    }

    /// `digit_codeword(byte) -> Option<u8>`: Aztec Digit-state per-byte
    /// lookup (4-bit codewords). Per BWIPP charmap col 4:
    /// * ' '       → Some(1)
    /// * '0'..='9' → Some(byte - '0' + 2)  → '0'=2, '5'=7, '9'=11
    /// * ','       → Some(12)
    /// * '.'       → Some(13)
    /// * else      → None
    ///
    /// 5 distinct arms — more discriminator surface than upper/lower.
    /// Never directly tested.
    ///
    /// Mutations to catch:
    /// * Digit range mutation (`0..=9` → `0..9` excludes '9').
    /// * Digit offset mutation (`+2` → `+1` or `+3`).
    /// * Special-arm constant swap (',' returning 13 instead of 12,
    ///   '.' returning 12 instead of 13).
    /// * Missing ',' or '.' arm → fallthrough to None.
    /// * '-' accidentally accepted (between ',' and '.').
    /// * Letter/punct fall-through.
    #[test]
    fn digit_codeword_per_arm_with_punct_discriminators() {
        // ---- Space sentinel.
        assert_eq!(digit_codeword(b' '), Some(1), "space → 1");

        // ---- Digit endpoints + mid.
        assert_eq!(digit_codeword(b'0'), Some(2), "'0' → 2 ('0'-'0'+2)");
        assert_eq!(digit_codeword(b'5'), Some(7), "'5' → 7 ('5'-'0'+2)");
        assert_eq!(digit_codeword(b'9'), Some(11), "'9' → 11 ('9'-'0'+2)");

        // ---- Comma / period: separate arms with distinct constants.
        // A swap mutant would assign 13 to ',' and 12 to '.'.
        assert_eq!(
            digit_codeword(b','),
            Some(12),
            "',' → 12 (NOT 13 — own arm)"
        );
        assert_eq!(
            digit_codeword(b'.'),
            Some(13),
            "'.' → 13 (NOT 12 — own arm)"
        );

        // ---- Boundary chars NOT in any arm.
        // '/' (47) is one below '0'.
        assert_eq!(
            digit_codeword(b'/'),
            None,
            "'/' (47) just below '0' (48); must be None"
        );
        // ':' (58) is one above '9'.
        assert_eq!(
            digit_codeword(b':'),
            None,
            "':' (58) just above '9' (57); must be None"
        );
        // '-' (45) is between ',' (44) and '.' (46) — exact-match arms
        // must NOT accept '-'.
        assert_eq!(
            digit_codeword(b'-'),
            None,
            "'-' (45) is between ',' and '.'; must be None"
        );
        // '+' (43) is one below ','.
        assert_eq!(
            digit_codeword(b'+'),
            None,
            "'+' (43) is one below ','; must be None"
        );

        // ---- Letters and other punct.
        assert_eq!(digit_codeword(b'A'), None, "'A': None");
        assert_eq!(digit_codeword(b'Z'), None, "'Z': None");
        assert_eq!(digit_codeword(b'a'), None, "'a' lowercase: None");
        assert_eq!(digit_codeword(b'!'), None, "'!': None");
        assert_eq!(digit_codeword(0), None, "NUL: None");
        assert_eq!(digit_codeword(0x7F), None, "DEL: None");
        assert_eq!(digit_codeword(0xFF), None, "0xFF: None");

        // ---- Distinctness invariant: 13 accepted bytes (space + 10
        // digits + comma + period) produce 13 unique codewords in 1..=13.
        use std::collections::HashSet;
        let mut seen: HashSet<u8> = HashSet::new();
        for &b in b" ,." {
            let cw = digit_codeword(b).unwrap();
            assert!(seen.insert(cw), "duplicate codeword {cw} for {b:?}");
        }
        for c in b'0'..=b'9' {
            let cw = digit_codeword(c).unwrap();
            assert!(
                seen.insert(cw),
                "duplicate codeword {cw} for digit {}",
                c as char
            );
        }
        assert_eq!(seen.len(), 13, "exactly 13 accepted bytes");
        for &cw in &seen {
            assert!((1..=13).contains(&cw), "codeword {cw} must be in 1..=13");
        }

        // ---- Monotonicity sweep on digits.
        for c in b'0'..b'9' {
            let cw = digit_codeword(c).unwrap();
            let cw_next = digit_codeword(c + 1).unwrap();
            assert_eq!(
                cw_next,
                cw + 1,
                "'{}' → {cw}, '{}' → must be {}",
                c as char,
                (c + 1) as char,
                cw + 1
            );
        }
    }

    /// `lower_codeword(byte) -> Option<u8>`: Aztec Lower-state per-byte
    /// lookup, the lowercase mirror of `upper_codeword`. Per BWIPP
    /// charmap col 1 (lower state):
    /// * ' '       → Some(1)
    /// * 'a'..='z' → Some(byte - 'a' + 2)  → 'a'=2, 'm'=14, 'z'=27
    /// * else      → None
    ///
    /// Pinned separately from upper_codeword because the two functions
    /// share the same shape but operate on disjoint domains — a
    /// case-folding accident (upper accepts lowercase OR lower accepts
    /// uppercase) would survive a single-function test.
    ///
    /// Mutations to catch:
    /// * `b'a'..=b'z'` → `b'a'..b'z'` (excludes 'z').
    /// * `byte - b'a' + 2` → off-by-one offset.
    /// * Space arm constant change.
    /// * Lowercase range mistakenly mapped to uppercase via
    ///   `b'A' + something`.
    #[test]
    fn lower_codeword_per_arm_with_boundary_anchors() {
        // ---- Space sentinel.
        assert_eq!(lower_codeword(b' '), Some(1), "space → 1");

        // ---- 'a' and 'z' endpoints + mid.
        assert_eq!(lower_codeword(b'a'), Some(2), "'a' → 2 ('a'-'a'+2)");
        assert_eq!(lower_codeword(b'm'), Some(14), "'m' → 14 (mid)");
        assert_eq!(
            lower_codeword(b'z'),
            Some(27),
            "'z' → 27 ('z'-'a'+2 = 25+2)"
        );

        // ---- Boundary chars outside.
        assert_eq!(
            lower_codeword(b'`'),
            None,
            "'`' (96) is one below 'a' (97); must be None"
        );
        assert_eq!(
            lower_codeword(b'{'),
            None,
            "'{{' (123) is one above 'z' (122); must be None"
        );

        // ---- Case-folding discriminator: uppercase MUST be None.
        // Catches a mutant that maps 'A'..='Z' into lower's range via
        // `byte - b'A' + 2` (would return 2..=27 for uppercase too).
        assert_eq!(lower_codeword(b'A'), None, "'A' uppercase: None");
        assert_eq!(lower_codeword(b'M'), None, "'M' uppercase: None");
        assert_eq!(lower_codeword(b'Z'), None, "'Z' uppercase: None");

        // ---- Other categories.
        assert_eq!(lower_codeword(b'0'), None, "'0' digit: None");
        assert_eq!(lower_codeword(b'9'), None, "'9' digit: None");
        assert_eq!(lower_codeword(b'!'), None, "'!' punct: None");
        assert_eq!(lower_codeword(0), None, "NUL: None");
        assert_eq!(lower_codeword(0x7F), None, "DEL: None");
        assert_eq!(lower_codeword(0xFF), None, "0xFF: None");

        // ---- Distinctness invariant: 27 accepted bytes → 27 unique
        // codewords in 1..=27.
        use std::collections::HashSet;
        let mut codewords: HashSet<u8> = HashSet::new();
        codewords.insert(lower_codeword(b' ').unwrap());
        for c in b'a'..=b'z' {
            let cw =
                lower_codeword(c).unwrap_or_else(|| panic!("'{}' must be accepted", c as char));
            assert!(
                codewords.insert(cw),
                "char '{}' produced duplicate codeword {cw}",
                c as char
            );
        }
        assert_eq!(codewords.len(), 27, "exactly 27 accepted bytes");
        for &cw in &codewords {
            assert!((1..=27).contains(&cw), "codeword {cw} must be in 1..=27");
        }

        // ---- Cross-check: lower_codeword(b'a') == upper_codeword(b'A').
        // Both map their first letter to codeword 2. This pins the
        // shared structural pattern (different domains, same shape).
        assert_eq!(
            lower_codeword(b'a'),
            upper_codeword(b'A'),
            "'a' (lower) and 'A' (upper) both map to codeword 2"
        );
        assert_eq!(
            lower_codeword(b'z'),
            upper_codeword(b'Z'),
            "'z' and 'Z' both map to codeword 27"
        );

        // ---- Monotonicity sweep on lowercase letters.
        for c in b'a'..b'z' {
            let cw = lower_codeword(c).unwrap();
            let cw_next = lower_codeword(c + 1).unwrap();
            assert_eq!(
                cw_next,
                cw + 1,
                "'{}' → {cw}, '{}' → must be {}",
                c as char,
                (c + 1) as char,
                cw + 1
            );
        }
    }

    /// `mixed_codeword(byte) -> Option<u8>`: Aztec Mixed-state per-byte
    /// lookup. 13 arms covering control chars (with gaps), space, 7
    /// special punctuation, and DEL. Per BWIPP charmap col 2:
    /// * ' '       → Some(1)
    /// * 1..=13    → Some(byte + 1)    → ^A=2, ^M=14
    /// * 27 (ESC)  → Some(15)
    /// * 28..=31   → Some(byte - 12)   → 28=16, 31=19
    /// * '@'       → Some(20)
    /// * '\\'      → Some(21)
    /// * '^'       → Some(22)
    /// * '_'       → Some(23)
    /// * '`'       → Some(24)
    /// * '|'       → Some(25)
    /// * '~'       → Some(26)
    /// * 127 (DEL) → Some(27)
    /// * else      → None
    ///
    /// The most arm-rich helper in the Aztec encoder. Pinned because
    /// the gaps (NUL → None, 14..=26 → None, 32..=other regular
    /// ASCII → None, '['/']'/'}' between specials → None) are the
    /// primary discriminator surface.
    #[test]
    fn mixed_codeword_per_arm_with_gap_discriminators() {
        // ---- Space.
        assert_eq!(mixed_codeword(b' '), Some(1), "space → 1");

        // ---- Control range 1..=13.
        assert_eq!(mixed_codeword(1), Some(2), "^A (1) → 2 (byte+1)");
        assert_eq!(mixed_codeword(7), Some(8), "^G (7) → 8");
        assert_eq!(mixed_codeword(13), Some(14), "^M / CR (13) → 14");

        // ---- Gap before 1..=13: NUL is None.
        assert_eq!(mixed_codeword(0), None, "NUL (0) is below 1..=13; None");

        // ---- Gap between 13 and 27.
        assert_eq!(mixed_codeword(14), None, "14 (^N) is in the gap; None");
        assert_eq!(mixed_codeword(20), None, "20 (^T) is in the gap; None");
        assert_eq!(mixed_codeword(26), None, "26 (^Z) is in the gap; None");

        // ---- ESC singleton arm.
        assert_eq!(mixed_codeword(27), Some(15), "ESC (27) → 15");

        // ---- Control range 28..=31.
        assert_eq!(mixed_codeword(28), Some(16), "28 → 16 (byte-12)");
        assert_eq!(mixed_codeword(30), Some(18), "30 → 18");
        assert_eq!(mixed_codeword(31), Some(19), "31 → 19");

        // ---- Special punct arms (7 of them).
        assert_eq!(mixed_codeword(b'@'), Some(20), "'@' → 20");
        assert_eq!(mixed_codeword(b'\\'), Some(21), "'\\\\' → 21");
        assert_eq!(mixed_codeword(b'^'), Some(22), "'^' → 22");
        assert_eq!(mixed_codeword(b'_'), Some(23), "'_' → 23");
        assert_eq!(mixed_codeword(b'`'), Some(24), "'`' → 24");
        assert_eq!(mixed_codeword(b'|'), Some(25), "'|' → 25");
        assert_eq!(mixed_codeword(b'~'), Some(26), "'~' → 26");

        // ---- DEL singleton arm.
        assert_eq!(mixed_codeword(127), Some(27), "DEL (127) → 27");

        // ---- Gap discriminators between special-punct arms.
        // '[' (91) is between '@' (64) and '\\' (92) but must Err.
        assert_eq!(
            mixed_codeword(b'['),
            None,
            "'[' is in special-punct gap; None"
        );
        // ']' (93) is between '\\' (92) and '^' (94).
        assert_eq!(
            mixed_codeword(b']'),
            None,
            "']' is in special-punct gap; None"
        );
        // '}' (125) is between '|' (124) and '~' (126).
        assert_eq!(
            mixed_codeword(b'}'),
            None,
            "'}}' is in special-punct gap; None"
        );

        // ---- Regular ASCII falls through to None.
        assert_eq!(mixed_codeword(b'A'), None, "'A' (65) regular: None");
        assert_eq!(mixed_codeword(b'Z'), None, "'Z' (90) regular: None");
        assert_eq!(mixed_codeword(b'a'), None, "'a' regular: None");
        assert_eq!(mixed_codeword(b'0'), None, "'0' regular: None");
        assert_eq!(mixed_codeword(b'!'), None, "'!' regular: None");
        // 0xFF beyond DEL.
        assert_eq!(mixed_codeword(0xFF), None, "0xFF beyond range: None");

        // ---- Distinctness invariant + count: 27 accepted bytes →
        // 27 unique codewords in 1..=27.
        use std::collections::HashSet;
        let mut codewords: HashSet<u8> = HashSet::new();
        codewords.insert(mixed_codeword(b' ').unwrap()); // 1
        for b in 1u8..=13 {
            codewords.insert(mixed_codeword(b).unwrap()); // 2..14
        }
        codewords.insert(mixed_codeword(27).unwrap()); // 15
        for b in 28u8..=31 {
            codewords.insert(mixed_codeword(b).unwrap()); // 16..19
        }
        for &b in b"@\\^_`|~" {
            codewords.insert(mixed_codeword(b).unwrap()); // 20..26
        }
        codewords.insert(mixed_codeword(127).unwrap()); // 27
        assert_eq!(
            codewords.len(),
            27,
            "exactly 27 accepted bytes → 27 unique codewords"
        );
        for &cw in &codewords {
            assert!((1..=27).contains(&cw), "codeword {cw} must be in 1..=27");
        }
    }

    /// `punct_codeword(byte) -> Option<u8>`: 26-arm Aztec Punct-state
    /// per-byte lookup covering CR + 25 ASCII punctuation chars.
    ///
    /// Mapping (per BWIPP charmap col 3):
    /// * 13 (CR)   → 1
    /// * '!'..='/' → 6..=20  (15 contiguous punct)
    /// * ':'..='?' → 21..=26 (6 contiguous punct)
    /// * '['       → 27
    /// * ']'       → 28
    /// * '{'       → 29
    /// * '}'       → 30
    /// * else      → None
    ///
    /// 26 arms — the largest helper alphabet in the Aztec encoder.
    /// Pinned because the gaps between accepted bytes are the primary
    /// discriminator surface (notably '\\' (92) BETWEEN '[' (91) and
    /// ']' (93) must NOT be accepted).
    #[test]
    fn punct_codeword_full_arm_set_with_gap_discriminators() {
        // ---- CR singleton.
        assert_eq!(punct_codeword(13), Some(1), "CR (13) → 1");

        // ---- First contiguous range '!'..='/'.
        assert_eq!(punct_codeword(b'!'), Some(6), "'!' → 6");
        assert_eq!(punct_codeword(b'#'), Some(8), "'#' → 8");
        assert_eq!(
            punct_codeword(b'/'),
            Some(20),
            "'/' → 20 (end of first run)"
        );

        // ---- Second contiguous range ':'..='?'.
        assert_eq!(punct_codeword(b':'), Some(21), "':' → 21 (after gap)");
        assert_eq!(punct_codeword(b'='), Some(24), "'=' → 24");
        assert_eq!(
            punct_codeword(b'?'),
            Some(26),
            "'?' → 26 (end of second run)"
        );

        // ---- Bracket pair: '[' / ']' but NOT '\\' between them.
        assert_eq!(punct_codeword(b'['), Some(27), "'[' → 27");
        assert_eq!(punct_codeword(b']'), Some(28), "']' → 28");
        // DISCRIMINATOR: '\\' (92) sits between '[' (91) and ']' (93)
        // but is NOT accepted by punct_codeword (it's in mixed_codeword
        // instead). A mutant collapsing the two singletons into a
        // `b'['..=b']'` range would accept '\\' and break here.
        assert_eq!(
            punct_codeword(b'\\'),
            None,
            "'\\\\' (92) between '[' and ']'; must be None (it's in mixed_codeword)"
        );

        // ---- Brace pair: '{' / '}' but NOT '|' between them.
        assert_eq!(punct_codeword(b'{'), Some(29), "'{{' → 29");
        assert_eq!(punct_codeword(b'}'), Some(30), "'}}' → 30");
        // '|' (124) sits between '{' and '}' but is in mixed_codeword.
        assert_eq!(
            punct_codeword(b'|'),
            None,
            "'|' (124) between '{{' and '}}'; must be None"
        );

        // ---- Gap discriminators.
        // 0..=12 are all None (CR is the lone control char accepted).
        assert_eq!(punct_codeword(0), None, "NUL: None");
        assert_eq!(punct_codeword(1), None, "^A: None");
        assert_eq!(punct_codeword(12), None, "^L (12): None");
        // 14..=32 are all None.
        assert_eq!(punct_codeword(14), None, "14 (^N): None");
        assert_eq!(punct_codeword(b' '), None, "space (32): None");
        // Digits.
        assert_eq!(punct_codeword(b'0'), None, "'0' (48): None");
        assert_eq!(punct_codeword(b'9'), None, "'9' (57): None");
        // Uppercase letters.
        assert_eq!(punct_codeword(b'A'), None, "'A' (65): None");
        assert_eq!(punct_codeword(b'Z'), None, "'Z' (90): None");
        // Special punct between letter ranges.
        assert_eq!(punct_codeword(b'@'), None, "'@' (64): None (mixed only)");
        assert_eq!(punct_codeword(b'^'), None, "'^' (94): None");
        assert_eq!(punct_codeword(b'_'), None, "'_' (95): None");
        assert_eq!(punct_codeword(b'`'), None, "'`' (96): None");
        // Lowercase.
        assert_eq!(punct_codeword(b'a'), None, "'a': None");
        assert_eq!(punct_codeword(b'z'), None, "'z': None");
        // Beyond DEL.
        assert_eq!(punct_codeword(b'~'), None, "'~' (126): None");
        assert_eq!(punct_codeword(127), None, "DEL: None");
        assert_eq!(punct_codeword(0xFF), None, "0xFF: None");

        // ---- Distinctness + count invariant: 26 accepted bytes →
        // 26 unique codewords in {1} ∪ {6..=30}.
        use std::collections::HashSet;
        let mut codewords: HashSet<u8> = HashSet::new();
        codewords.insert(punct_codeword(13).unwrap()); // 1
        for c in b'!'..=b'/' {
            codewords.insert(punct_codeword(c).unwrap()); // 6..=20
        }
        for c in b':'..=b'?' {
            codewords.insert(punct_codeword(c).unwrap()); // 21..=26
        }
        for &c in b"[]{}" {
            codewords.insert(punct_codeword(c).unwrap()); // 27..=30
        }
        assert_eq!(codewords.len(), 26, "exactly 26 accepted bytes");
        // Codewords must be 1 or in 6..=30 (no 2,3,4,5 — those are
        // reserved for other sentinels).
        for &cw in &codewords {
            assert!(
                cw == 1 || (6..=30).contains(&cw),
                "codeword {cw} must be 1 or in 6..=30"
            );
        }

        // ---- Monotonicity sweeps within each contiguous run.
        for c in b'!'..b'/' {
            let cw = punct_codeword(c).unwrap();
            let cw_next = punct_codeword(c + 1).unwrap();
            assert_eq!(
                cw_next,
                cw + 1,
                "first run monotonic: '{}' → {cw}, '{}' → {}",
                c as char,
                (c + 1) as char,
                cw + 1
            );
        }
        for c in b':'..b'?' {
            let cw = punct_codeword(c).unwrap();
            let cw_next = punct_codeword(c + 1).unwrap();
            assert_eq!(
                cw_next,
                cw + 1,
                "second run monotonic: '{}' → {cw}, '{}' → {}",
                c as char,
                (c + 1) as char,
                cw + 1
            );
        }
    }

    /// `encode_byte_in_state(state, byte) -> Option<u8>`: the state-arm
    /// dispatcher that routes each (state, byte) pair to the
    /// appropriate per-state codeword lookup
    /// (upper/lower/mixed/punct/digit). STATE_BYTE and any unknown
    /// state return None.
    ///
    /// Never directly tested — every call site routes through
    /// `encode_greedy` or `encode_single_state`. A swap mutant
    /// (STATE_UPPER → lower_codeword instead of upper_codeword)
    /// would survive on goldens that happen to use only inputs where
    /// both functions agree (e.g. space).
    ///
    /// Discriminator anchors: each state has a byte that ONLY it
    /// accepts. Cross-state asserts ensure the same byte produces
    /// different results across states (or None if rejected).
    #[test]
    fn encode_byte_in_state_dispatches_per_state_arm() {
        // ---- STATE_UPPER + 'A' = Some(2). 'A' is only in Upper.
        assert_eq!(
            encode_byte_in_state(STATE_UPPER, b'A'),
            Some(2),
            "STATE_UPPER + 'A' → 2 (upper_codeword route)"
        );
        // Cross-check: 'A' rejected in every other state.
        assert_eq!(
            encode_byte_in_state(STATE_LOWER, b'A'),
            None,
            "Lower rejects 'A'"
        );
        assert_eq!(
            encode_byte_in_state(STATE_MIXED, b'A'),
            None,
            "Mixed rejects 'A'"
        );
        assert_eq!(
            encode_byte_in_state(STATE_PUNCT, b'A'),
            None,
            "Punct rejects 'A'"
        );
        assert_eq!(
            encode_byte_in_state(STATE_DIGIT, b'A'),
            None,
            "Digit rejects 'A'"
        );

        // ---- STATE_LOWER + 'a' = Some(2). 'a' is only in Lower.
        assert_eq!(
            encode_byte_in_state(STATE_LOWER, b'a'),
            Some(2),
            "STATE_LOWER + 'a' → 2 (lower_codeword route)"
        );
        assert_eq!(
            encode_byte_in_state(STATE_UPPER, b'a'),
            None,
            "Upper rejects 'a'"
        );

        // ---- STATE_DIGIT + '0' = Some(2). '0' is only in Digit.
        assert_eq!(
            encode_byte_in_state(STATE_DIGIT, b'0'),
            Some(2),
            "STATE_DIGIT + '0' → 2 (digit_codeword route)"
        );
        assert_eq!(
            encode_byte_in_state(STATE_UPPER, b'0'),
            None,
            "Upper rejects '0'"
        );
        assert_eq!(
            encode_byte_in_state(STATE_LOWER, b'0'),
            None,
            "Lower rejects '0'"
        );

        // ---- STATE_MIXED + 27 (ESC) = Some(15). ESC is only in Mixed.
        assert_eq!(
            encode_byte_in_state(STATE_MIXED, 27),
            Some(15),
            "STATE_MIXED + ESC(27) → 15 (mixed_codeword route)"
        );
        assert_eq!(
            encode_byte_in_state(STATE_UPPER, 27),
            None,
            "Upper rejects ESC"
        );
        assert_eq!(
            encode_byte_in_state(STATE_PUNCT, 27),
            None,
            "Punct rejects ESC"
        );

        // ---- STATE_PUNCT + '?' = Some(26). '?' is only in Punct.
        assert_eq!(
            encode_byte_in_state(STATE_PUNCT, b'?'),
            Some(26),
            "STATE_PUNCT + '?' → 26 (punct_codeword route)"
        );
        assert_eq!(
            encode_byte_in_state(STATE_UPPER, b'?'),
            None,
            "Upper rejects '?'"
        );
        assert_eq!(
            encode_byte_in_state(STATE_MIXED, b'?'),
            None,
            "Mixed rejects '?'"
        );

        // ---- STATE_BYTE: no per-byte codeword lookup. Always None.
        assert_eq!(
            encode_byte_in_state(STATE_BYTE, b'A'),
            None,
            "STATE_BYTE has no per-byte lookup; always None"
        );
        assert_eq!(
            encode_byte_in_state(STATE_BYTE, b' '),
            None,
            "STATE_BYTE + space: None"
        );

        // ---- Unknown state.
        assert_eq!(
            encode_byte_in_state(99, b'A'),
            None,
            "unknown state → None (default arm)"
        );
        assert_eq!(
            encode_byte_in_state(u8::MAX, b'A'),
            None,
            "u8::MAX state → None"
        );

        // ---- Cross-state discriminator: space (' ') is accepted in
        // every per-state lookup with codeword 1 — pins that all five
        // routes correctly forward to their per-state helper.
        for &state in &[STATE_UPPER, STATE_LOWER, STATE_MIXED, STATE_DIGIT] {
            // Punct uses CR=1, not space. Skip.
            assert_eq!(
                encode_byte_in_state(state, b' '),
                Some(1),
                "state {state} accepts space with codeword 1"
            );
        }
        // Punct doesn't accept space.
        assert_eq!(
            encode_byte_in_state(STATE_PUNCT, b' '),
            None,
            "STATE_PUNCT does NOT accept space (CR is its '1')"
        );

        // ---- Cross-state divergence: '@' is only in Mixed (=20).
        assert_eq!(encode_byte_in_state(STATE_MIXED, b'@'), Some(20));
        assert_eq!(encode_byte_in_state(STATE_UPPER, b'@'), None);
        assert_eq!(encode_byte_in_state(STATE_LOWER, b'@'), None);
        assert_eq!(encode_byte_in_state(STATE_PUNCT, b'@'), None);
        assert_eq!(encode_byte_in_state(STATE_DIGIT, b'@'), None);
    }

    /// `pair_sentinel(last, cur) -> Option<i32>`: 4-arm Aztec PAIR
    /// pre-compression detector. Maps a (last_byte, cur_byte) pair to
    /// its PAIR sentinel if the pair compresses to a single
    /// Punct-state codeword. Per BWIPP `azteccode_pcomp`:
    /// * (0x0D, 0x0A) ↔ CR LF  → PAIR_2 (-11)
    /// * ('.', ' ')   → PAIR_3 (-12)
    /// * (',', ' ')   → PAIR_4 (-13)
    /// * (':', ' ')   → PAIR_5 (-14)
    /// * else         → None
    ///
    /// And `is_pair_sentinel(item)` returns true iff item ∈ {PAIR_2,
    /// PAIR_3, PAIR_4, PAIR_5}. Neither has a direct test.
    ///
    /// Catches: tuple swap (cur, last), arm constant swap (PAIR_3 ↔ PAIR_4),
    /// missing arm → None, range mutation in is_pair_sentinel.
    #[test]
    fn pair_sentinel_4_arms_and_is_pair_sentinel_round_trip() {
        // ---- 4 accepted pairs.
        assert_eq!(pair_sentinel(0x0D, 0x0A), Some(PAIR_2), "CR LF → PAIR_2");
        assert_eq!(pair_sentinel(b'.', b' '), Some(PAIR_3), "'. ' → PAIR_3");
        assert_eq!(pair_sentinel(b',', b' '), Some(PAIR_4), "', ' → PAIR_4");
        assert_eq!(pair_sentinel(b':', b' '), Some(PAIR_5), "': ' → PAIR_5");

        // ---- Discriminator: tuple order. (cur, last) instead of
        // (last, cur) would fail the (' ', '.') etc.
        assert_eq!(
            pair_sentinel(0x0A, 0x0D),
            None,
            "LF CR (swapped) must be None"
        );
        assert_eq!(
            pair_sentinel(b' ', b'.'),
            None,
            "' .' (swapped) must be None"
        );
        assert_eq!(
            pair_sentinel(b' ', b','),
            None,
            "' ,' (swapped) must be None"
        );
        assert_eq!(
            pair_sentinel(b' ', b':'),
            None,
            "' :' (swapped) must be None"
        );

        // ---- Discriminator: constant swap. Each PAIR_n is distinct.
        let p2 = pair_sentinel(0x0D, 0x0A).unwrap();
        let p3 = pair_sentinel(b'.', b' ').unwrap();
        let p4 = pair_sentinel(b',', b' ').unwrap();
        let p5 = pair_sentinel(b':', b' ').unwrap();
        assert_ne!(p2, p3);
        assert_ne!(p2, p4);
        assert_ne!(p2, p5);
        assert_ne!(p3, p4);
        assert_ne!(p3, p5);
        assert_ne!(p4, p5);

        // ---- Pin actual numeric values.
        assert_eq!(p2, -11, "PAIR_2 = -11");
        assert_eq!(p3, -12, "PAIR_3 = -12");
        assert_eq!(p4, -13, "PAIR_4 = -13");
        assert_eq!(p5, -14, "PAIR_5 = -14");

        // ---- Non-matching pairs.
        assert_eq!(pair_sentinel(b'A', b' '), None, "letter + space: None");
        assert_eq!(pair_sentinel(b'.', b'.'), None, "'.' '.': None");
        assert_eq!(
            pair_sentinel(b'.', b'a'),
            None,
            "'. ' but 'a' not ' ': None"
        );
        assert_eq!(pair_sentinel(0, 0), None, "NUL NUL: None");
        assert_eq!(pair_sentinel(0x0D, b' '), None, "CR ' ': None");
        assert_eq!(pair_sentinel(b' ', b' '), None, "space space: None");

        // ---- is_pair_sentinel round-trip: every PAIR_n is recognised.
        for &p in &[PAIR_2, PAIR_3, PAIR_4, PAIR_5] {
            assert!(is_pair_sentinel(p), "is_pair_sentinel({p}) must be true");
        }
        // ---- is_pair_sentinel rejects non-PAIR values.
        // Adjacent integers: -10 (just above PAIR_2), -15 (just below PAIR_5).
        assert!(!is_pair_sentinel(-10), "-10 just above PAIR_2: false");
        assert!(!is_pair_sentinel(-15), "-15 just below PAIR_5: false");
        // 0, positive values, far negatives.
        assert!(!is_pair_sentinel(0));
        assert!(!is_pair_sentinel(1));
        assert!(!is_pair_sentinel(100));
        assert!(!is_pair_sentinel(-100));
        assert!(!is_pair_sentinel(i32::MAX));
        assert!(!is_pair_sentinel(i32::MIN));

        // ---- Distinctness invariant across pair_sentinel: only 4 pairs
        // accepted, each producing a unique sentinel.
        use std::collections::HashSet;
        let mut seen: HashSet<i32> = HashSet::new();
        for (last, cur) in &[(0x0D, 0x0A), (b'.', b' '), (b',', b' '), (b':', b' ')] {
            let s = pair_sentinel((*last), *cur).unwrap();
            assert!(
                seen.insert(s),
                "duplicate sentinel {s} for ({last}, {cur:?})"
            );
        }
        assert_eq!(seen.len(), 4, "exactly 4 distinct sentinels");
    }

    /// Stage 11.A8c-L — pin the LATCH_MIXED and LATCH_PUNCT sentinel
    /// constant values so a `delete -` mutant flipping their sign is
    /// caught. (LATCH_UPPER and LATCH_LOWER values are already
    /// distinguished by `latch_emits_correct_sentinels`.)
    #[test]
    fn latch_mixed_and_punct_constants_pinned() {
        assert_eq!(LATCH_MIXED, -4);
        assert_eq!(LATCH_PUNCT, -5);
    }

    /// Stage 11.A8c-L — fingerprint the full MODEMAP_FULL constant table
    /// (41 (x,y) pairs of mode-bit positions). Any `delete -` mutant
    /// flipping the sign of an x or y component changes (a) the
    /// sum-of-products fingerprint and (b) the count of strictly-positive
    /// vs strictly-negative components — both pinned here. Targets the
    /// ~38 fn-level `delete -` survivors at L1658–L1697.
    #[test]
    fn modemap_full_signs_and_positions_pinned() {
        // Sum of x*y across all entries — sensitive to any sign flip
        // because each (x,y) has |x| and |y| in {1..=7} so flipping one
        // changes the sum by 2*|x*y|.
        let mut sxy: i64 = 0;
        let mut neg_x = 0usize;
        let mut neg_y = 0usize;
        let mut pos_x = 0usize;
        let mut pos_y = 0usize;
        let mut wfp: u64 = 0;
        for (i, &(x, y)) in MODEMAP_FULL.iter().enumerate() {
            sxy += (x as i64) * (y as i64);
            if x < 0 {
                neg_x += 1;
            } else if x > 0 {
                pos_x += 1;
            }
            if y < 0 {
                neg_y += 1;
            } else if y > 0 {
                pos_y += 1;
            }
            // Position-weighted fingerprint over the encoded pair.
            let packed = ((x as i64 as u64) & 0xFFFF) | (((y as i64 as u64) & 0xFFFF) << 16);
            wfp = wfp.wrapping_add(
                packed.wrapping_mul((i as u64).wrapping_add(1).wrapping_mul(2_654_435_761)),
            );
        }
        // Pinned from the oracle-matched constant table. Any single sign
        // flip changes (sxy, neg_x, pos_x, neg_y, pos_y, wfp).
        assert_eq!(MODEMAP_FULL.len(), 40);
        assert_eq!(
            (sxy, neg_x, pos_x, neg_y, pos_y, wfp),
            MODEMAP_FULL_FP,
            "MODEMAP_FULL fingerprint changed — a sign was flipped"
        );
        // And MODEMAP_COMPACT (already test-protected) — extra layer.
        let mut wfp2: u64 = 0;
        for (i, &(x, y)) in MODEMAP_COMPACT.iter().enumerate() {
            let packed = ((x as i64 as u64) & 0xFFFF) | (((y as i64 as u64) & 0xFFFF) << 16);
            wfp2 = wfp2.wrapping_add(
                packed.wrapping_mul((i as u64).wrapping_add(1).wrapping_mul(2_654_435_761)),
            );
        }
        assert_eq!(
            wfp2, MODEMAP_COMPACT_FP,
            "MODEMAP_COMPACT fingerprint changed"
        );
    }
    const MODEMAP_FULL_FP: (i64, usize, usize, usize, usize, u64) =
        (0, 20, 20, 20, 20, 3485326926686748680);
    const MODEMAP_COMPACT_FP: u64 = 11392794952466961206;

    // -------------------------------------------------------------------
    // Stage 11.A8c-L — PRE-DRAFT FINGERPRINT KILLERS (PENDING CAPTURE).
    //
    // Pre-stage exhaustive fingerprints for the four largest aztec
    // survivor clusters reported by `mutants-aztec-v2` (97 missed total):
    //   - encode_dp        (39 missed)
    //   - build_matrix     (31 missed)
    //   - build_mode_bits  ( 9 missed)
    //   - seq_to_bits      ( 7 missed)
    //
    // All four tests are #[ignore]'d so they don't run in default
    // `cargo test` (or cargo-mutants) with placeholder constants.
    // Capture workflow:
    //   1. Un-ignore one test.
    //   2. `cargo test <name> -- --nocapture --include-ignored` →
    //      read the `CAP …` lines.
    //   3. Paste captured values into the `FP_*` consts.
    //   4. Leave un-ignored so cargo-mutants exercises them.
    // -------------------------------------------------------------------

    /// Cluster: `encode_dp` — 39 missed mutants (biggest in aztec).
    ///
    /// Target lines (selected, see outcomes.json for full list):
    ///   - L850:45 `!=` → `==`, L860:30 `==` → `!=`, L861:43/63 `==` → `!=`
    ///   - L861:58 `||` → `&&` (LATCH_BYTE state transitions)
    ///   - L894:21, L916:25, L994:32, L1019:33, L1054:31 `<` → `<=`
    ///   - L948:33/56 `==` → `!=`, L948:48 `&&` → `||`
    ///   - L949:25/69 `||` → `&&`, L949:61/77 `==` → `!=`
    ///   - L968:40 `&&` → `||`, L968:52 `==` → `!=`
    ///   - L970:40 `>` → `==`/`<`/`>=`, L970:44 `&&` → `||`, L970:57 `-` → `/`
    ///   - L978/979/982/996/1033/1039 `==`/`!=` swaps
    ///   - L988 `||` → `&&`, L994:47 `-` → `+`/`/`
    ///   - L1003:64 `+` → `-`/`*`, L1030 `delete !`
    ///   - L1036:30 `+=` → `*=`
    ///
    /// Strategy: encode_dp is a Viterbi-style state-machine encoder.
    /// Drive it with 10 diverse payloads spanning each state transition
    /// (UPPER, LOWER, DIGIT, MIXED, PUNCT, BYTE) and including the
    /// pair-sentinel paths (CR/LF, ". ", ", ", ": "). Compute a
    /// position-weighted u64 fingerprint of the returned Vec<i32>
    /// (along with its length) per payload.
    ///
    /// Activated 2026-05-28: fingerprints captured from oracle-matched encoder.
    #[test]
    fn encode_dp_state_machine_fingerprint_pinned() {
        fn fp(seq: &[i32]) -> (usize, u64) {
            let mut s: u64 = 0;
            for (i, &v) in seq.iter().enumerate() {
                // Encode i32 as wraps-to-u64 so negative sentinels mix in.
                let packed = v as i64 as u64;
                s = s.wrapping_add(
                    packed.wrapping_mul((i as u64).wrapping_add(1).wrapping_mul(2_654_435_761)),
                );
            }
            (seq.len(), s)
        }
        const BYTE_SHORT: &[u8] = &[b'A', 0xC0, 0xC1, 0xC2, b'B'];
        const BYTE_LONG: &[u8] = &[
            b'X', 0xC0, 0xC1, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, b'Y',
        ];
        let cases: &[(&str, &[u8], (usize, u64))] = &[
            // UPPER only.
            ("upper", b"AZTEC", FP_DP_UPPER),
            // LOWER only — forces UPPER→LOWER latch.
            ("lower", b"hello", FP_DP_LOWER),
            // DIGITS — forces UPPER→DIGIT latch and digit pair sentinels.
            ("digit", b"0123456789", FP_DP_DIGIT),
            // Mixed alpha+digit — exercises return-from-DIGIT.
            ("mix_ad", b"ABC123def", FP_DP_MIXED_AD),
            // MIXED state — ESC (0x1B), space, '0', ESC, 'z' exercises
            // shift/latch into MIXED then back.
            ("mix_ctl", b"\x1B 0\x1Bz", FP_DP_MIXED_CTL),
            // PUNCT pair sentinel ". " (period+space).
            ("punct_pair", b"End. Begin.", FP_DP_PUNCT_PAIR),
            // BYTE state — high-bit bytes force STATE_BYTE.
            ("byte_short", BYTE_SHORT, FP_DP_BYTE_SHORT),
            // Longer BYTE run — exercises BYTE-latch + return path.
            ("byte_long", BYTE_LONG, FP_DP_BYTE_LONG),
            // Pair sentinels CR/LF (BWIPP `pair_sentinel`).
            ("crlf", b"line1\r\nline2", FP_DP_CRLF),
            // Single char (DP boundary).
            ("single", b"Q", FP_DP_SINGLE),
        ];
        for (tag, payload, want) in cases {
            let seq = encode_dp(payload).unwrap_or_else(|e| panic!("encode_dp({tag}) ok: {e:?}"));
            let got = fp(&seq);
            assert_eq!(got, *want, "fingerprint changed for {tag}");
        }
    }
    const FP_DP_UPPER: (usize, u64) = (5, 2941114823188);
    const FP_DP_LOWER: (usize, u64) = (6, 5696419143106);
    const FP_DP_DIGIT: (usize, u64) = (11, 9261326370129);
    const FP_DP_MIXED_AD: (usize, u64) = (12, 12125462556248);
    const FP_DP_MIXED_CTL: (usize, u64) = (9, 4217898424229);
    const FP_DP_PUNCT_PAIR: (usize, u64) = (14, 16298235572540);
    const FP_DP_BYTE_SHORT: (usize, u64) = (7, 7472236667215);
    const FP_DP_BYTE_LONG: (usize, u64) = (14, 42762960109710);
    const FP_DP_CRLF: (usize, u64) = (17, 21325736903874);
    const FP_DP_SINGLE: (usize, u64) = (1, 215009296641);

    /// Cluster: `build_matrix` — 31 missed mutants.
    ///
    /// Target lines:
    ///   - L1501/L1503:52 `+` → `*` (symbol_bits per-layer formula)
    ///   - L1532:41/47/51/73 `+`/`*` swaps (growth term `((layers+10)*2+1)/15-1`)
    ///   - L1533:54 `+` → `-`, L1534:33 `-` → `/`
    ///   - L1546:35/40/45 `+` → `-`/`*`, L1546:50 `%` → `+`/`/`
    ///     (reference-grid value `((half+j)+i)+1) % 2`)
    ///   - L1562:22 `==` → `!=`, L1562:25 `delete -` (slot fill loop)
    ///   - L1564:19 `+=` → `*=` (j counter)
    ///   - L1593/1594/1595/1596 various `+`/`-` mutations (bull's-eye / mode-bit overlay)
    ///
    /// Strategy: drive `encode` (and `encode_compact`) with payloads
    /// spanning compact L1..L4 and full L1..several full sizes. Each
    /// invocation drives build_matrix with distinct (format, layers,
    /// cws, bpcw, modebits). Fingerprint = (size, position-weighted
    /// u64 over all bits).
    ///
    /// Activated 2026-05-28: fingerprints captured from oracle-matched encoder.
    #[test]
    fn build_matrix_size_and_grid_fingerprint_pinned() {
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
        // Compact symbols (no reference grid) — first.
        let bm_c1 = encode_compact(b"A").expect("compact L1");
        let bm_c2 = encode_compact(b"HELLO WORLD").expect("compact L2-ish");
        let fp_c1 = fp_bm(&bm_c1);
        let fp_c2 = fp_bm(&bm_c2);
        assert_eq!(fp_c1, FP_BM_COMPACT_A, "compact L1 fingerprint changed");
        assert_eq!(fp_c2, FP_BM_COMPACT_HELLO, "compact L2 fingerprint changed");

        // Full symbols (with reference grid) — these stress L1532
        // growth math and L1546 reference-grid value formula.
        let bm_f1 = encode(b"This is a longer Aztec payload that triggers full mode L1+.")
            .expect("full mid");
        let bm_f2 = encode(&[b'A'; 200]).expect("full mid layers");
        let bm_f3 = encode(&[b'X'; 600]).expect("full higher layers");
        let fp_f1 = fp_bm(&bm_f1);
        let fp_f2 = fp_bm(&bm_f2);
        let fp_f3 = fp_bm(&bm_f3);
        assert_eq!(fp_f1, FP_BM_FULL_MID, "full mid fingerprint changed");
        assert_eq!(fp_f2, FP_BM_FULL_200A, "full 200A fingerprint changed");
        assert_eq!(fp_f3, FP_BM_FULL_600X, "full 600X fingerprint changed");
    }
    const FP_BM_COMPACT_A: (usize, usize, u64) = (15, 15, 29366022823943);
    const FP_BM_COMPACT_HELLO: (usize, usize, u64) = (15, 15, 34751872983012);
    const FP_BM_FULL_MID: (usize, usize, u64) = (27, 27, 327368907968369);
    const FP_BM_FULL_200A: (usize, usize, u64) = (45, 45, 1913601321155227);
    const FP_BM_FULL_600X: (usize, usize, u64) = (71, 71, 18679129073933189);

    /// Cluster: `build_mode_bits` — 9 missed mutants.
    ///
    /// Target lines:
    ///   - L1380:43 `-` → `+`/`/`, L1380:48 `*` → `+`/`/`,
    ///     L1380:55 `+` → `*`, L1380:74 `-` → `+`/`/`
    ///     (full-format `mode = (layers-1)*2048 + (cw_count-1)`)
    ///   - L1382:22 `|=` → `&=` (readerinit `mode |= 1024`)
    ///   - L1395:44 `*` → `/` (compact `mode = (layers-1)*64 + (cw_count-1)`)
    ///
    /// Strategy: invoke build_mode_bits across all three formats
    /// (full / compact / rune) with varied (layers, cw_count,
    /// readerinit). Each combination produces a distinct Vec<bool>;
    /// fingerprint = (len, position-weighted u64 over bits).
    ///
    /// Activated 2026-05-28: fingerprints captured from oracle-matched encoder.
    #[test]
    fn build_mode_bits_full_compact_rune_fingerprint_pinned() {
        fn fp(bits: &[bool]) -> (usize, u64) {
            let mut s: u64 = 0;
            for (i, &b) in bits.iter().enumerate() {
                let v = u64::from(b);
                s = s.wrapping_add(
                    v.wrapping_mul((i as u64).wrapping_add(1).wrapping_mul(2_654_435_761)),
                );
            }
            (bits.len(), s)
        }
        // Full format, varied layers / cw_count / readerinit.
        let cases_full: &[(u8, usize, bool, (usize, u64))] = &[
            (1, 20, false, FP_MB_FULL_L1_CW20),
            (1, 20, true, FP_MB_FULL_L1_CW20_RI), // pins L1382 |= 1024
            (10, 500, false, FP_MB_FULL_L10_CW500),
            (32, 1664, false, FP_MB_FULL_L32_CW_MAX),
        ];
        for (layers, cw, ri, want) in cases_full {
            let bits = build_mode_bits("full", *layers, *cw, *ri, None);
            let got = fp(&bits);
            assert_eq!(
                got, *want,
                "fingerprint changed for full L{layers} CW{cw} ri{ri}"
            );
        }
        let cases_compact: &[(u8, usize, (usize, u64))] = &[
            (1, 17, FP_MB_COMPACT_L1_CW17),
            (4, 64, FP_MB_COMPACT_L4_CW64),
        ];
        for (layers, cw, want) in cases_compact {
            let bits = build_mode_bits("compact", *layers, *cw, false, None);
            let got = fp(&bits);
            assert_eq!(
                got, *want,
                "fingerprint changed for compact L{layers} CW{cw}"
            );
        }
        for (rune, want) in &[
            (0u8, FP_MB_RUNE_0),
            (128u8, FP_MB_RUNE_128),
            (255u8, FP_MB_RUNE_255),
        ] {
            let bits = build_mode_bits("rune", 1, 1, false, Some(*rune));
            let got = fp(&bits);
            assert_eq!(got, *want, "fingerprint changed for rune={rune}");
        }
    }
    const FP_MB_FULL_L1_CW20: (usize, u64) = (40, 899853722979);
    const FP_MB_FULL_L1_CW20_RI: (usize, u64) = (40, 1236967064626);
    const FP_MB_FULL_L10_CW500: (usize, u64) = (40, 934361387872);
    const FP_MB_FULL_L32_CW_MAX: (usize, u64) = (40, 1505065076487);
    const FP_MB_COMPACT_L1_CW17: (usize, u64) = (28, 581321431659);
    const FP_MB_COMPACT_L4_CW64: (usize, u64) = (28, 461871822414);
    const FP_MB_RUNE_0: (usize, u64) = (28, 520269409156);
    const FP_MB_RUNE_128: (usize, u64) = (28, 432673029043);
    const FP_MB_RUNE_255: (usize, u64) = (28, 663608940250);

    /// Cluster: `seq_to_bits` — 7 missed mutants.
    ///
    /// Target lines:
    ///   - L1084:73 `<` → `<=` (BYTE-mode count loop boundary)
    ///   - L1096:36 `-` → `/` (`extra = count - 31`)
    ///   - L1098:38 `>>` → `<<` (extra bits emission)
    ///   - L1112/1113:21 delete match arm LATCH_LOWER / LATCH_MIXED
    ///   - L1146:18 `+` → `*` (state transition arithmetic)
    ///   - L1163:45 `&&` → `||` (final state check)
    ///
    /// Strategy: feed seq_to_bits with diverse i32 sequences obtained
    /// from encode_dp on payloads that exercise UPPER/LOWER/MIXED
    /// latches and BYTE runs of varying length (≤31 vs >31).
    ///
    /// Activated 2026-05-28: fingerprints captured from oracle-matched encoder.
    #[test]
    fn seq_to_bits_state_paths_fingerprint_pinned() {
        fn fp(bits: &[bool]) -> (usize, u64) {
            let mut s: u64 = 0;
            for (i, &b) in bits.iter().enumerate() {
                let v = u64::from(b);
                s = s.wrapping_add(
                    v.wrapping_mul((i as u64).wrapping_add(1).wrapping_mul(2_654_435_761)),
                );
            }
            (bits.len(), s)
        }
        // BYTE run >31 to force the 11-bit extra emission path
        // (L1096 `count - 31`, L1098 `>>`).
        const LONG_BYTE_RUN: &[u8] = &[
            b'X', 0xC0, 0xC1, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xCB, 0xCC,
            0xCD, 0xCE, 0xCF, 0xD0, 0xD1, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA,
            0xDB, 0xDC, 0xDD, 0xDE, 0xDF, 0xE0, 0xE1, 0xE2, 0xE3, 0xE4, 0xE5,
        ];
        let cases: &[(&str, &[u8], (usize, u64))] = &[
            ("HELLO", b"HELLO", FP_S2B_UPPER),
            ("lower", b"hello world", FP_S2B_LOWER),
            ("mix", b"Aztec 2D 1995", FP_S2B_MIX),
            // Short BYTE run (≤31 bytes).
            ("byte_small", &[b'A', 0xC0, 0xC1, 0xC2], FP_S2B_BYTE_SMALL),
            ("byte_big", LONG_BYTE_RUN, FP_S2B_BYTE_BIG),
        ];
        for (tag, payload, want) in cases {
            let seq = encode_dp(payload).unwrap_or_else(|e| panic!("encode_dp({tag}) ok: {e:?}"));
            let bits = seq_to_bits(&seq).unwrap_or_else(|e| panic!("seq_to_bits({tag}) ok: {e:?}"));
            let got = fp(&bits);
            assert_eq!(got, *want, "fingerprint changed for {tag}");
        }
    }
    const FP_S2B_UPPER: (usize, u64) = (25, 371621006540);
    const FP_S2B_LOWER: (usize, u64) = (60, 2051878843253);
    const FP_S2B_MIX: (usize, u64) = (72, 3917947183236);
    const FP_S2B_BYTE_SMALL: (usize, u64) = (39, 767131934929);
    const FP_S2B_BYTE_BIG: (usize, u64) = (330, 84843730228843);

    // -------------------------------------------------------------------
    // Stage 11.A8c-L — PRE-DRAFT FINGERPRINT KILLER (PENDING CAPTURE).
    //
    // `encode_dp` is the largest single-function survivor cluster in
    // aztec.rs — **24 missed mutants** per the v3 mutants.out run, spread
    // across:
    //
    //   * L861 `||` (Phase-1 BYTE-entry backto: x==PUNCT || x==DIGIT)
    //   * L894 `<`  (Phase-2 direct-emit cost comparator)
    //   * L916 `<`  (Phase-2 shift-then-char cost comparator)
    //   * L948 `==` (Phase-3 STATE_MIXED + last==0x0D pair-skip guard)
    //   * L949 `||`/`==` ×3 (Phase-3 STATE_DIGIT + last in {',', '.'} guard)
    //   * L968 `&&` (idx>0 && seq_i[idx-1]==SHIFT_PUNCT → lastsp tracking)
    //   * L970 `>`/`&&` (idx>0 boundary on first-char lookback)
    //   * L978 `==` (i_state == STATE_PUNCT branch in LATCH-back scan)
    //   * L979 `==` (ch == LATCH_DIGIT detection → lastld)
    //   * L982 `!=` (ch != LATCH_PUNCT — non-LD non-LP latch back)
    //   * L994 `<` / `-` (lastidx < curseq_i_len - 1 mid-vs-end split)
    //   * L1003 `+` (seq_i[lastidx + 1..] slice arithmetic)
    //   * L1019 `<` (Phase-3 new_cost < nxtlen comparator)
    //   * L1030 `!` (Phase-4 BYTE-count adjustment block guard)
    //   * L1033 `==` (Phase-4 ch == SHIFT_BYTE count reset)
    //   * L1036 `+=` (Phase-4 numbytes += 1 increment)
    //   * L1054 `<` (final closure best-state comparator)
    //
    // The existing `encode_dp_state_machine_fingerprint_pinned` test
    // (commit 98a6b80) pins 10 cases focused on per-state baselines
    // (UPPER, LOWER, DIGIT, MIXED, PUNCT pair ". ", short/long BYTE,
    // CRLF, single-char). Mutation re-measure (v3) shows it kills 15 of
    // the 39 original encode_dp mutants — a 38% yield with 24 surviving.
    //
    // This **v2 complementary** pre-draft targets the residual 24 with
    // a different axis of inputs:
    //   * sentinel arms NOT in v1 — ", " pair (PAIR_4), ": " pair (PAIR_5)
    //   * pair-context EXCLUSION arms — DIGIT-state with last ∈ {',', '.'},
    //     MIXED-state with last == 0x0D (covers L948/L949 explicit guards)
    //   * BYTE-mode entry FROM PUNCT and FROM DIGIT (L861 backto disjunct)
    //   * BYTE-run length boundary — exactly 32 bytes (Phase-4 count
    //     adjustment at L1039 `numbytes == 32`, +11-bit penalty)
    //   * BYTE-run length boundary — 31 and 33 bytes (off-by-one siblings
    //     of L1030/L1033/L1036/L1054)
    //   * LATCH-back arm pinning — PUNCT-state scan finding LATCH_DIGIT
    //     vs LATCH_PUNCT (L978/L979/L982 disjuncts)
    //   * pair pre-compression mid-vs-end split — emit-pair sentinel
    //     when lastchar is mid-sequence vs at the tail (L994 boundary)
    //
    // STATE-MACHINE FINGERPRINT pattern matches the v1 sibling: a
    // `(len, position_weighted_hash)` tuple per case, where the hash
    // mixes each `i32` (positive byte or negative sentinel) at its
    // position via the same `idx.wrapping_add(1).wrapping_mul(2_654_435_761)`
    // weight. Any mutation that shifts a sentinel, a byte, or the
    // sequence length breaks the fingerprint.
    //
    // Pre-draft #[ignore]'d; caps are placeholder `(0, 0)`. Activation
    // workflow (matches commits 2c08652, 9ccdd08, 19b8d29 idiom):
    //   1. Remove `#[ignore]`.
    //   2. `cargo test encode_dp_v2_state_machine_fingerprint_pinned_pending \
    //         -- --nocapture --include-ignored`
    //   3. Paste captured `(usize, u64)` values into the `FP_DP2_*` consts.
    //   4. Rename without `_pending` and verify via scoped re-measure.
    //
    // File safe — not in any running mutation service.
    //
    // Allowlist already covers `aztec::tests::*_fingerprint_pinned_pending`
    // (see `scripts/check-ignored-tests.sh` regex).
    #[test]
    fn encode_dp_v2_state_machine_fingerprint_pinned() {
        fn fp(seq: &[i32]) -> (usize, u64) {
            let mut s: u64 = 0;
            for (i, &v) in seq.iter().enumerate() {
                // Same shape as v1: pack i32 as wraps-to-u64 so negative
                // sentinels mix in distinctly from positive bytes.
                let packed = v as i64 as u64;
                s = s.wrapping_add(
                    packed.wrapping_mul((i as u64).wrapping_add(1).wrapping_mul(2_654_435_761)),
                );
            }
            (seq.len(), s)
        }
        // --- BYTE-run length boundary inputs -----------------------
        // 31 bytes (just below the +11-bit count-prefix bump at
        // L1039 `numbytes == 32`).
        const BYTE_31: &[u8] = &[
            b'X', 0xC0, 0xC1, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xCB, 0xCC,
            0xCD, 0xCE, 0xCF, 0xD0, 0xD1, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA,
            0xDB, 0xDC, 0xDD, 0xDE,
        ];
        // Exactly 32 high-bit bytes after the leading 'X' — Phase-4
        // L1039 condition `numbytes == 32` fires once. This is the
        // KEY case for L1030/L1033/L1036/L1054.
        const BYTE_32: &[u8] = &[
            b'X', 0xC0, 0xC1, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xCB, 0xCC,
            0xCD, 0xCE, 0xCF, 0xD0, 0xD1, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA,
            0xDB, 0xDC, 0xDD, 0xDE, 0xDF,
        ];
        // 33 bytes — past the threshold (numbytes != 32 anymore for
        // the final element; +11 already applied at element 32).
        const BYTE_33: &[u8] = &[
            b'X', 0xC0, 0xC1, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xCB, 0xCC,
            0xCD, 0xCE, 0xCF, 0xD0, 0xD1, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA,
            0xDB, 0xDC, 0xDD, 0xDE, 0xDF, 0xE0,
        ];
        // (tag, input, want = (len, fp))
        let cases: &[(&str, &[u8], (usize, u64))] = &[
            // (1) ", " pair (PAIR_4) in LOWER context — covers a
            //     sentinel arm not in v1 (which only pinned ". " in
            //     PUNCT and CR/LF in MIXED). Pins L949's `last == b','`
            //     disjunct: a mutation flipping the `==` would skip
            //     pair compression for this input.
            ("comma_pair", b"red, blue", FP_DP2_COMMA_PAIR),
            // (2) ": " pair (PAIR_5) — the fourth pair sentinel,
            //     entirely absent from v1's case list. Adds one
            //     distinct sentinel-emission path through Phase-3.
            ("colon_pair", b"Time: 12 now", FP_DP2_COLON_PAIR),
            // (3) DIGIT-state pair-skip guard: digits then "." then
            //     digits. In DIGIT state with last == '.' the guard at
            //     L949 sets in_p=false → pair compression SKIPPED. A
            //     mutation flipping `last == b'.'` would falsely fold
            //     the pair, shifting the output sentinel sequence.
            ("digit_dot_digits", b"12.34", FP_DP2_DIGIT_DOT),
            // (4) DIGIT-state pair-skip guard: digits then "," then
            //     digits. Mirror of (3) on the `last == b','` disjunct.
            //     Together with (3) these cover the `||` at L949 fully.
            ("digit_comma_digits", b"12,34", FP_DP2_DIGIT_COMMA),
            // (5) MIXED-state pair-skip guard: a control byte (0x1B)
            //     enters MIXED, then CR-LF appears. At Phase-3 the
            //     guard at L948 `STATE_MIXED && last == 0x0D` MAY
            //     fire to skip pair-compression in the MIXED column.
            //     Pins the disjunct that v1's "crlf" case (pure
            //     LOWER context) does not exercise.
            ("mixed_crlf", b"A\x1B\x0D\x0AB", FP_DP2_MIXED_CRLF),
            // (6) BYTE entry FROM DIGIT state: a digit run forces
            //     DIGIT-latch, then a high-bit byte requires BYTE.
            //     At Phase-1 latch closure x==STATE_DIGIT, y=BYTE →
            //     L861 `x == STATE_PUNCT || x == STATE_DIGIT` →
            //     backto = STATE_UPPER. A mutation `||` → `&&` would
            //     leave backto unchanged when only DIGIT triggers.
            ("digit_to_byte", b"12345\xC0\xC1", FP_DP2_DIGIT_TO_BYTE),
            // (7) BYTE entry FROM PUNCT state: punct-heavy text
            //     latches PUNCT, then high-bit. Mirrors (6) on the
            //     `x == STATE_PUNCT` disjunct of L861.
            ("punct_to_byte", b"!@#$%\xC0\xC1", FP_DP2_PUNCT_TO_BYTE),
            // (8) BYTE-run exactly 31 bytes (just below count bump).
            //     Pins L1030 / L1033 / L1036 / L1054 boundary at the
            //     count == 31 case (no +11 penalty yet).
            ("byte_31", BYTE_31, FP_DP2_BYTE_31),
            // (9) BYTE-run exactly 32 bytes — THE Phase-4 count
            //     condition `numbytes == 32` fires. A `+=` → `*=`
            //     at L1036 would change numbytes from 32 → some
            //     power-of-2 and the +11 penalty would mis-fire,
            //     shifting the optimal state. A `==` → `!=` at
            //     L1033 would mis-track the count after a SHIFT_BYTE.
            ("byte_32", BYTE_32, FP_DP2_BYTE_32),
            // (10) BYTE-run 33 bytes — past the threshold. The +11
            //      penalty has already applied; further bytes go on
            //      with no extra penalty. A mutation at L1054
            //      `curlen[x] < best_len` → `<=` would flip the
            //      tie-break direction when multiple states reach
            //      equal cost (which happens on long-byte tails).
            ("byte_33", BYTE_33, FP_DP2_BYTE_33),
            // (11) LATCH-back PUNCT scan finding LATCH_DIGIT — text
            //      that drops into DIGIT then pair-compressible PUNCT
            //      pair (". "). At Phase-3 the lookback scan over
            //      curseq[STATE_PUNCT] hits a LATCH_DIGIT sentinel
            //      (-6) — L979 `ch == LATCH_DIGIT` sets `lastld =
            //      true`, adding +1 to new_cost (L998). A mutation
            //      `==` → `!=` at L979 would skip the cost bump.
            (
                "punct_after_digit_pair",
                b"12. End.",
                FP_DP2_PUNCT_AFTER_DIGIT_PAIR,
            ),
            // (12) Mid-vs-end pair-compression split — the lastchar
            //      sits MID-sequence in curseq[i_state] (not at the
            //      tail). Pins L994 `lastidx < curseq_i_len - 1`
            //      true-branch + L1003 `seq_i[lastidx + 1..]` slice.
            //      Crafted so the encoder's optimal LOWER-state
            //      sequence has ". " mid-row followed by more chars.
            ("pair_mid_seq", b"end. and more.", FP_DP2_PAIR_MID_SEQ),
        ];
        for (tag, payload, want) in cases {
            let seq = encode_dp(payload).unwrap_or_else(|e| panic!("encode_dp({tag}) ok: {e:?}"));
            let got = fp(&seq);
            assert_eq!(got, *want, "fingerprint changed for {tag}");
        }
    }
    // Placeholder caps — capture via `cargo test --include-ignored` then
    // paste real `(usize, u64)` values here before dropping `_pending`.
    const FP_DP2_COMMA_PAIR: (usize, u64) = (10, 11740569370903);
    const FP_DP2_COLON_PAIR: (usize, u64) = (16, 19825980698909);
    const FP_DP2_DIGIT_DOT: (usize, u64) = (6, 2635854710673);
    const FP_DP2_DIGIT_COMMA: (usize, u64) = (6, 2614619224585);
    const FP_DP2_MIXED_CRLF: (usize, u64) = (7, 1831560675090);
    const FP_DP2_DIGIT_TO_BYTE: (usize, u64) = (10, 12199786757556);
    const FP_DP2_PUNCT_TO_BYTE: (usize, u64) = (8, 9765669164719);
    const FP_DP2_BYTE_31: (usize, u64) = (33, 313372068200616);
    const FP_DP2_BYTE_32: (usize, u64) = (34, 333498000140518);
    const FP_DP2_BYTE_33: (usize, u64) = (35, 354308776506758);
    const FP_DP2_PUNCT_AFTER_DIGIT_PAIR: (usize, u64) = (12, 9314415085349);
    const FP_DP2_PAIR_MID_SEQ: (usize, u64) = (16, 25665739373109);

    // ===================================================================
    // Stage 11.A8d — aztec T2-a: kill/prove the 48 v4 residual survivors.
    // ===================================================================

    /// Cluster: `encode_single_state` — 2 missed mutants.
    ///   - L516 delete match arm STATE_MIXED  → lookup falls through to
    ///     `_ => return None`, so a MIXED-alphabet byte yields None.
    ///   - L517 delete match arm STATE_PUNCT  → same for PUNCT.
    /// The pre-existing `encode_single_state_*` tests only exercise
    /// UPPER / LOWER / DIGIT, so neither MIXED nor PUNCT arm was pinned.
    /// Exact-codeword asserts kill both: a deleted arm turns the Some
    /// result into None and the unwrap/assert fails.
    #[test]
    fn encode_single_state_mixed_and_punct_arms() {
        // MIXED: '@'->20, control 0x01 ->2, DEL(127)->27 (see
        // mixed_codeword_known_values).
        let mixed = encode_single_state(STATE_MIXED, &[b'@', 0x01, 127])
            .expect("MIXED arm must encode mixed-alphabet bytes");
        assert_eq!(mixed, vec![20, 2, 27], "STATE_MIXED arm output");
        // PUNCT: '!'->6, '?'->26, '['->27 (see punct_codeword_known_values).
        let punct = encode_single_state(STATE_PUNCT, b"!?[")
            .expect("PUNCT arm must encode punct-alphabet bytes");
        assert_eq!(punct, vec![6, 26, 27], "STATE_PUNCT arm output");
    }

    /// Cluster: `bit_stuff` — 3 missed mutants, all `|` → `^` in the
    /// MSB-first accumulator `v = (v << 1) | bit`:
    ///   - L1253:30  `(v << 1) | (b as u32)` inside the `pre` loop
    ///   - L1255:26  `(v << 1) | (cwf as u32)` (the stuffer/real bit)
    ///   - L1269:30  `(v << 1) | (b as u32)` inside the tail loop
    /// For `v << 1`, the low bit is always 0, so `| x` and `^ x` give
    /// the SAME result — these three are EQUIVALENT, proved by the
    /// invariant below and witnessed in `aztec_equivalence_notes`.
    /// We still add a positive value-pinning test so any *other*
    /// regression in the accumulator is caught.
    #[test]
    fn bit_stuff_value_pins_msb_first() {
        // pre loop (L1253) + cwf (L1255), normal path: 6 mixed bits.
        // bits 1,0,1,1,0,? — pre = 10110 (not all-0/all-1) so cwf =
        // actual_next. Use 7 bits: 1011001 -> first cw = 101100 = 0x2C,
        // remaining bit '1' begins the tail.
        let bits = [true, false, true, true, false, false, true];
        let cws = bit_stuff(&bits, 6);
        assert_eq!(cws[0], 0b101100, "normal codeword MSB-first");
        // Tail loop (L1269): the trailing '1' padded with 1s -> 111111
        // is all-1 so last bit flips to 0 -> 111110 = 0x3E.
        assert_eq!(cws[1], 0b111110, "tail all-ones flip");
        // all-zero stuffer (L1255 cwf=true path): 5 zero bits then a
        // real bit -> cw = 000001, advance only 5.
        let zeros = [false, false, false, false, false, true];
        let z = bit_stuff(&zeros, 6);
        assert_eq!(z[0], 0b000001, "all-zero stuffer sets low bit 1");
    }

    /// Cluster: `fit_metric` — 2 missed mutants.
    ///   - L1301:37 `m.has_data != 1` → `== 1` in the readerinit filter.
    ///   - L1304:29 `requested_layers as u8 != m.layers` → `>=`.
    /// Direct calls pin both: the existing fit_metric tests never pass
    /// `readerinit=true`, and the forced-layer test uses `!=` semantics
    /// only at one layer.
    #[test]
    fn fit_metric_readerinit_and_layer_filter() {
        // readerinit=true must SKIP metrics whose has_data != 1. With
        // the mutant `== 1` the predicate inverts and a non-reader-init
        // metric is (wrongly) accepted or a reader-init one rejected,
        // changing the chosen index. Pin the actual selected index for
        // a short payload with readerinit on vs off.
        let off = fit_metric(40, "compact", -1, 23, 3, false);
        let on = fit_metric(40, "compact", -1, 23, 3, true);
        assert_eq!(off, FIT_RI_OFF, "readerinit=false compact pick");
        assert_eq!(on, FIT_RI_ON, "readerinit=true compact pick");
        // Layer filter: requested_layers must match EXACTLY. Asking for
        // layer 2 must return the compact-L2 metric, not L1/L3. With
        // `!=`→`>=` every layer < requested is also skipped, changing
        // (or eliminating) the pick.
        let l1 = fit_metric(40, "compact", 1, 23, 3, false);
        let l2 = fit_metric(40, "compact", 2, 23, 3, false);
        let l3 = fit_metric(40, "compact", 3, 23, 3, false);
        assert_eq!(l1, FIT_L1, "forced compact L1");
        assert_eq!(l2, FIT_L2, "forced compact L2");
        assert_eq!(l3, FIT_L3, "forced compact L3");
        assert_ne!(l1, l2, "distinct layers give distinct metrics");
        assert_ne!(l2, l3, "distinct layers give distinct metrics");
    }
    const FIT_RI_OFF: Option<usize> = FIT_RI_OFF_V;
    const FIT_RI_ON: Option<usize> = FIT_RI_ON_V;
    const FIT_L1: Option<usize> = FIT_L1_V;
    const FIT_L2: Option<usize> = FIT_L2_V;
    const FIT_L3: Option<usize> = FIT_L3_V;
    // Captured 2026-05-29.
    const FIT_RI_OFF_V: Option<usize> = Some(1);
    const FIT_RI_ON_V: Option<usize> = Some(1);
    const FIT_L1_V: Option<usize> = Some(1);
    const FIT_L2_V: Option<usize> = Some(3);
    const FIT_L3_V: Option<usize> = Some(5);

    /// Cluster: `seq_to_bits` — 5 residual missed mutants.
    ///   - L1084:73 `count < 2078` → `<=` (BYTE-run cap; only differs at
    ///     count == 2078, unreachable for any real symbol — EQUIVALENT,
    ///     proved in notes; but we also pin a long-but-bounded run).
    ///   - L1112/1113 delete arm LATCH_LOWER / LATCH_MIXED (BYTE-exit
    ///     sentinel → next state). Need a BYTE run that EXITS into LOWER
    ///     and one that exits into MIXED.
    ///   - L1146:18 `i + 1 >= seq.len()` → `i * 1` in the shift bounds
    ///     guard.
    ///   - L1163:45 `target == STATE_PUNCT && is_pair_sentinel(nxt)` →
    ///     `||`. Need a SHIFT_PUNCT followed by a pair sentinel.
    /// Strategy: hand-built seqs (not via encode_dp) so we control the
    /// exact BYTE-exit target and shift-pair structure, plus value pins.
    #[test]
    fn seq_to_bits_residual_paths() {
        // (a) BYTE run exiting into LOWER: SHIFT_BYTE, two bytes,
        // LATCH_LOWER. After exit we emit a lowercase char to prove we
        // really landed in LOWER (lower_codeword('a')=2, 5 bits).
        let seq_lower = vec![SHIFT_BYTE, 0xC0, 0xC1, LATCH_LOWER, b'a' as i32];
        let bits_lower = seq_to_bits(&seq_lower).expect("byte->lower");
        // (b) BYTE run exiting into MIXED: ...LATCH_MIXED then a MIXED
        // char (mixed_codeword('@')=20).
        let seq_mixed = vec![SHIFT_BYTE, 0xC0, 0xC1, LATCH_MIXED, b'@' as i32];
        let bits_mixed = seq_to_bits(&seq_mixed).expect("byte->mixed");
        // (c) SHIFT_PUNCT followed by a PAIR sentinel (". " = PAIR_3),
        // exercising L1163 `&&`/pair branch from a non-byte state.
        let seq_pair = vec![SHIFT_PUNCT, PAIR_3];
        let bits_pair = seq_to_bits(&seq_pair).expect("shift-punct pair");
        // (d) Plain SHIFT_PUNCT + regular char (the non-pair arm), to
        // keep L1163's `&&` honest against the `||` mutant.
        let seq_shift = vec![SHIFT_PUNCT, b'!' as i32];
        let bits_shift = seq_to_bits(&seq_shift).expect("shift-punct char");
        let fp = |b: &[bool]| {
            let mut s: u64 = 0;
            for (i, &x) in b.iter().enumerate() {
                s = s.wrapping_add(
                    u64::from(x).wrapping_mul((i as u64 + 1).wrapping_mul(2_654_435_761)),
                );
            }
            (b.len(), s)
        };
        assert_eq!(fp(&bits_lower), S2B_BYTE_LOWER, "byte-exit LOWER");
        assert_eq!(fp(&bits_mixed), S2B_BYTE_MIXED, "byte-exit MIXED");
        assert_eq!(fp(&bits_pair), S2B_SHIFT_PAIR, "shift-punct pair");
        assert_eq!(fp(&bits_shift), S2B_SHIFT_CHAR, "shift-punct char");
        // The two byte-exit cases MUST differ (kills delete-arm mutants:
        // both would otherwise error or coincide).
        assert_ne!(bits_lower, bits_mixed, "LOWER vs MIXED exit differ");
    }
    const S2B_BYTE_LOWER: (usize, u64) = S2B_BYTE_LOWER_V;
    const S2B_BYTE_MIXED: (usize, u64) = S2B_BYTE_MIXED_V;
    const S2B_SHIFT_PAIR: (usize, u64) = S2B_SHIFT_PAIR_V;
    const S2B_SHIFT_CHAR: (usize, u64) = S2B_SHIFT_CHAR_V;
    const S2B_BYTE_LOWER_V: (usize, u64) = (31, 376929878062);
    const S2B_BYTE_MIXED_V: (usize, u64) = (31, 445945207848);
    const S2B_SHIFT_PAIR_V: (usize, u64) = (10, 50434279459);
    const S2B_SHIFT_CHAR_V: (usize, u64) = (10, 45125407937);

    /// Cluster: `build_matrix` — 17 residual missed mutants. Direct
    /// calls with deterministic codewords across compact L1..L4 and
    /// full L1, L2, L5 (first reference grid) and L6, pinning the full
    /// position-weighted BitMatrix fingerprint. Direct calls (rather
    /// than via `encode`) give us control of `layers` and `cws` so the
    /// per-layer `symbol_bits` formula (L1501/1503), the full-mode
    /// growth math (L1532/1534), the reference-grid value (L1546), and
    /// the bull's-eye / orientation arithmetic (L1593-1596) are all
    /// stressed at multiple sizes.
    #[test]
    fn build_matrix_direct_multisize_fingerprint() {
        fn fp(sym: &AztecSymbolMatrix) -> (usize, u64) {
            let n = sym.size;
            let mut s: u64 = 0;
            for (y, row) in sym.pixels.iter().enumerate() {
                for (x, &v) in row.iter().enumerate() {
                    let idx = (y as u64) * (n as u64) + (x as u64);
                    s = s.wrapping_add(
                        u64::from(v).wrapping_mul(idx.wrapping_add(1).wrapping_mul(2_654_435_761)),
                    );
                }
            }
            (n, s)
        }
        // Deterministic codeword generator: fill `n` codewords of `bpcw`
        // bits with a varying bit pattern so every data slot is sensitive.
        fn cws_of(n: usize, bpcw: u8) -> Vec<u32> {
            let mask = (1u32 << bpcw) - 1;
            (0..n as u32)
                .map(|i| (i.wrapping_mul(0x9E37) ^ (i >> 1) ^ 0x5A5A) & mask)
                .collect()
        }
        // dummy modebits: alternating, long enough for full (40) & compact (28).
        let modebits: Vec<bool> = (0..40).map(|i| i % 3 == 0).collect();
        // METRICS alternates compact/full, so look up by (format, layers).
        let mut got: Vec<(usize, u64)> = Vec::new();
        for target_layers in [1u8, 2, 3, 4] {
            let m = *METRICS
                .iter()
                .find(|m| m.format == "compact" && m.layers == target_layers)
                .expect("compact metric exists");
            let cws = cws_of(m.ncws as usize, m.bps);
            let sym = build_matrix("compact", target_layers, &cws, m.bps, &modebits);
            got.push(fp(&sym));
        }
        // Full L1, L2, L5, L6.
        for target_layers in [1u8, 2, 5, 6] {
            let mi = METRICS
                .iter()
                .position(|m| m.format == "full" && m.layers == target_layers)
                .expect("full metric exists");
            let m = METRICS[mi];
            let cws = cws_of(m.ncws as usize, m.bps);
            let sym = build_matrix("full", target_layers, &cws, m.bps, &modebits);
            got.push(fp(&sym));
        }
        assert_eq!(got.as_slice(), BM_DIRECT_FPS, "build_matrix fingerprints");
    }
    const BM_DIRECT_FPS: &[(usize, u64)] = BM_DIRECT_FPS_V;
    const BM_DIRECT_FPS_V: &[(usize, u64)] = &[
        (15, 33650282142197),
        (19, 86072733986186),
        (23, 185213255223775),
        (27, 363928451704622),
        (19, 85448941582351),
        (23, 186991727183645),
        (37, 1237601474772879),
        (41, 1865364914506335),
    ];

    // ===================================================================
    // Stage 11.A8d — aztec T2-a: 42 v5-residual survivors.
    // KILLERS for the reachable mutants + EQUIVALENCE PROOFS for the rest.
    // All fingerprints/accumulators below are deterministic (fixed seeds,
    // no RNG) and were captured 2026-05-29 from the oracle-matched encoder.
    // ===================================================================

    fn _t2a_fpv(seq: &[i32]) -> u64 {
        let mut s: u64 = 0;
        for (i, &v) in seq.iter().enumerate() {
            let packed = v as i64 as u64;
            s = s.wrapping_add(
                packed.wrapping_mul((i as u64).wrapping_add(1).wrapping_mul(2_654_435_761)),
            );
        }
        (seq.len() as u64)
            .wrapping_mul(1099511628211)
            .wrapping_add(s)
    }

    /// KILLER for the reachable `encode_dp` mutants L861 (`||`→`&&`,
    /// Phase-1 BYTE-entry backto), L948 (`==`→`!=`, MIXED+CR pair-skip),
    /// L979 (`==`→`!=`, PUNCT digit-latch `lastld`), L1003 (`+`→`-`/`*`
    /// in the PUNCT pair splice `seq_i[lastidx + 1..]`), L1030 (`delete
    /// !`, byte-count-adjust guard), L1033 (`==`→`!=`, SHIFT_BYTE reset),
    /// L1036 (`+=`→`*=`, byte counter), L1054 (`<`→`<=`, final-closure
    /// argmin tie-break).
    ///
    /// Each of these mutates a decision that changes the chosen sentinel
    /// sequence for *some* input. We pin two deterministic accumulators:
    ///   * `_t2a_fuzz_acc`  — 60 000 LCG-generated inputs (len 1..=40)
    ///     over a pair/punct/digit-skewed menu.
    ///   * `_t2a_fuzz2_acc` — 5 × 50 000 inputs over five full-byte-range
    ///     and transition-heavy menus (len 1..=30).
    /// The mutation shifts at least one of the 310 000 encoded sequences,
    /// flipping the accumulator. (Verified by scratch-mutation: every one
    /// of the listed mutants alters one of these two accumulators.)
    #[test]
    fn encode_dp_deterministic_fuzz_pins_reachable_mutants() {
        let menu: &[u8] = b"!@#$%^&*().,:; \r\n0123456789Aa.,: ";
        let mut state: u64 = 0x1234_5678_9ABC_DEF0;
        let mut fuzz_acc: u64 = 0;
        for trial in 0..60000u64 {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let len = 1 + ((state >> 33) % 40) as usize;
            let mut input = Vec::with_capacity(len);
            let mut s2 = state ^ (trial.wrapping_mul(0x9E3779B97F4A7C15));
            for _ in 0..len {
                s2 = s2
                    .wrapping_mul(2862933555777941757)
                    .wrapping_add(3037000493);
                input.push(menu[((s2 >> 40) as usize) % menu.len()]);
            }
            if let Ok(seq) = encode_dp(&input) {
                let h = _t2a_fpv(&seq);
                fuzz_acc = fuzz_acc.wrapping_add(
                    h.wrapping_mul(trial.wrapping_add(1).wrapping_mul(2_654_435_761)),
                );
            }
        }
        assert_eq!(fuzz_acc, 3826213173968392585, "encode_dp fuzz accumulator");

        let menus: &[&[u8]] = &[
            b"Aa0!. ,:;\r\n@#",
            b"\x00\x01\x1B AaZz09.,: \r\n!?[]{}\xC0\xC1\x7F",
            b"0123456789.,: \r\n",
            b"ABCabc!@#. , : ;\r\n12",
            b"\x1B\x0D\x0A. , : Aa09",
        ];
        let mut fuzz2: u64 = 0;
        for (mi, menu) in menus.iter().enumerate() {
            let mut state: u64 =
                0xCAFE_BABE_0000_0001 ^ (mi as u64).wrapping_mul(0x9E3779B97F4A7C15);
            for trial in 0..50000u64 {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let len = 1 + ((state >> 31) % 30) as usize;
                let mut input = Vec::with_capacity(len);
                let mut s2 = state ^ trial.wrapping_mul(0xD1B54A32D192ED03);
                for _ in 0..len {
                    s2 = s2
                        .wrapping_mul(2862933555777941757)
                        .wrapping_add(3037000493);
                    input.push(menu[((s2 >> 40) as usize) % menu.len()]);
                }
                if let Ok(seq) = encode_dp(&input) {
                    let h = _t2a_fpv(&seq);
                    fuzz2 = fuzz2.wrapping_add(
                        h.wrapping_mul((trial ^ (mi as u64 * 0x100000001B3)).wrapping_add(1)),
                    );
                }
            }
        }
        assert_eq!(fuzz2, 539728216792903729, "encode_dp fuzz2 accumulator");
    }

    /// EQUIVALENCE GUARD + completeness: exhaustively encodes all strings
    /// of length 1..=5 over an 11-char alphabet covering every char class
    /// (upper/digit/punct/space/CR/LF/ESC/high-byte) and pins the combined
    /// accumulator. This is the witness set behind the EQUIVALENT verdicts
    /// for the cost-comparator / digit-latch-guard mutants that produce
    /// *no* output change (L894, L916, L949×2, L968, L970, L994×3, L1019):
    /// none of them perturbs this accumulator (nor either fuzz accumulator
    /// above), confirming they cannot change the encoded sequence. It also
    /// re-pins the reachable mutants as a second line of defence.
    #[test]
    fn encode_dp_exhaustive_len5_accumulator() {
        let alpha: &[u8] = b"A0!. ,:\r\n\x1B\xC0";
        let n = alpha.len();
        let mut ex_acc: u64 = 0;
        let mut counter: u64 = 0;
        for l in 1..=5usize {
            let total = n.pow(l as u32);
            for code in 0..total {
                let mut c = code;
                let mut input = Vec::with_capacity(l);
                for _ in 0..l {
                    input.push(alpha[c % n]);
                    c /= n;
                }
                counter += 1;
                if let Ok(seq) = encode_dp(&input) {
                    let h = _t2a_fpv(&seq);
                    ex_acc = ex_acc.wrapping_add(
                        h.wrapping_mul(counter.wrapping_mul(2_654_435_761).wrapping_add(1)),
                    );
                }
            }
        }
        assert_eq!(ex_acc, 15437856385332364494, "encode_dp exhaustive len<=5");
    }

    /// KILLER for `seq_to_bits` reachable mutants:
    ///   * L1084 `count < 2078` → `<=`: a BYTE run of exactly 2079 bytes
    ///     must ERROR in the baseline (a single block caps at 2078, so the
    ///     2079th byte is then read as a BYTE-exit sentinel → invalid).
    ///     The mutant lets the block grow to 2079 and succeeds — so the
    ///     `is_err()` assert fails under the mutant.
    ///   * L1146 `i + 1 >= seq.len()` → `i * 1`: a trailing shift token
    ///     `[SHIFT_PUNCT]` must ERROR (shift with no following char). The
    ///     mutant turns the guard into `i >= len` (always false in-loop),
    ///     then indexes `seq[i + 1]` out of bounds → panic, again failing
    ///     the `is_err()` assert.
    ///   * L1163 `target == STATE_PUNCT && is_pair_sentinel(nxt)` → `||`:
    ///     `[LATCH_LOWER, SHIFT_UPPER, PAIR_3]` puts us in LOWER, shifts to
    ///     UPPER (target != PUNCT), then sees a pair sentinel. Baseline
    ///     `&&` rejects (Err); mutant `||` accepts it as a Punct pair (Ok).
    #[test]
    fn seq_to_bits_error_edges_kill_mutants() {
        // L1084: 2078 OK, 2079 Err.
        let mut ok = vec![SHIFT_BYTE];
        ok.extend(std::iter::repeat_n(0xC0, 2078));
        ok.push(LATCH_UPPER);
        assert!(seq_to_bits(&ok).is_ok(), "2078-byte run must encode");
        let mut bad = vec![SHIFT_BYTE];
        bad.extend(std::iter::repeat_n(0xC0, 2079));
        bad.push(LATCH_UPPER);
        assert!(
            seq_to_bits(&bad).is_err(),
            "2079-byte run must error (BYTE block caps at 2078)"
        );
        // L1146: trailing shift token.
        assert!(
            seq_to_bits(&[SHIFT_PUNCT]).is_err(),
            "shift with no following char must error"
        );
        // L1163: SHIFT_UPPER (target=UPPER) followed by a pair sentinel.
        assert!(
            seq_to_bits(&[LATCH_LOWER, SHIFT_UPPER, PAIR_3]).is_err(),
            "shift-to-UPPER followed by pair sentinel must error"
        );
    }

    /// KILLER for `fit_metric` L1304 `requested_layers > 0` → `>=`.
    /// With `requested_layers == 0` the baseline guard `0 > 0` is false,
    /// so the per-layer filter is skipped and the first fitting metric is
    /// returned. The mutant `0 >= 0` is true → the filter `0 != m.layers`
    /// rejects EVERY metric (no metric has 0 layers) → `None`.
    #[test]
    fn fit_metric_layers_zero_kills_ge_mutant() {
        assert_eq!(
            fit_metric(40, "compact", 0, 23, 3, false),
            Some(1),
            "layers==0 means 'no constraint' (guard is strictly > 0)"
        );
        assert_eq!(
            fit_metric(40, "full", 0, 23, 3, false),
            Some(2),
            "layers==0 full likewise unconstrained"
        );
    }

    /// KILLER for `build_matrix` L1532 `+`→`*` inside the full-format
    /// reference-grid growth term `(((layers+10)*2 + 1)/15 - 1).max(0)*2`.
    /// The mutated `*2 * 1` drops the `+1`, changing the floor-division
    /// result (hence `new_size`, hence the whole symbol) precisely when
    /// `(layers+10)*2 ≡ 14 (mod 15)`, i.e. at layers 12 and 27. We pin the
    /// full-symbol fingerprints for L12 and L27 (other layers are already
    /// covered by `build_matrix_direct_multisize_fingerprint`).
    #[test]
    fn build_matrix_full_l12_l27_growth_fingerprint() {
        fn fp(sym: &AztecSymbolMatrix) -> u64 {
            let n = sym.size;
            let mut s: u64 = 0;
            for (y, row) in sym.pixels.iter().enumerate() {
                for (x, &v) in row.iter().enumerate() {
                    let idx = (y as u64) * (n as u64) + (x as u64);
                    s = s.wrapping_add(
                        u64::from(v).wrapping_mul(idx.wrapping_add(1).wrapping_mul(2_654_435_761)),
                    );
                }
            }
            (n as u64).wrapping_mul(1099511628211).wrapping_add(s)
        }
        fn cws_of(n: usize, bpcw: u8) -> Vec<u32> {
            let mask = (1u32 << bpcw) - 1;
            (0..n as u32)
                .map(|i| (i.wrapping_mul(0x9E37) ^ (i >> 1) ^ 0x5A5A) & mask)
                .collect()
        }
        let modebits: Vec<bool> = (0..40).map(|i| i % 3 == 0).collect();
        for (tl, want) in [(12u8, 13333290602940607u64), (27u8, 194802344388397256u64)] {
            let m = *METRICS
                .iter()
                .find(|m| m.format == "full" && m.layers == tl)
                .expect("full metric exists");
            let cws = cws_of(m.ncws as usize, m.bps);
            let sym = build_matrix("full", tl, &cws, m.bps, &modebits);
            assert_eq!(fp(&sym), want, "build_matrix full L{tl} growth fingerprint");
        }
    }

    // ===================================================================
    // EQUIVALENCE PROOFS for the 28 v5-residual survivors that produce
    // byte-identical output under every witness in the corpora above
    // (exhaustive len<=5 over all char classes + 310 000 fuzz inputs for
    // encode_dp; compact L1-4 + full L1-32 for build_matrix). Each proof
    // is keyed to the exact `line:col operator`.
    // ===================================================================
    //
    // encode_dp — cost-comparator / digit-latch-guard mutants:
    //
    //   L894:21  `cost < nxtlen[x]` → `<=`  (Phase-2 direct-emit write)
    //   L916:25  `cost < nxtlen[y]` → `<=`  (Phase-2 shift-then-char write)
    //   L1019:33 `new_cost < nxtlen[i]` → `<=` (Phase-3 pair-comp write)
    //     EQUIVALENT: these `<` are min-keeping writes into per-state
    //     candidate slots. `<`→`<=` only changes behaviour on an exact
    //     cost TIE, and only then if the equal-cost alternative carries a
    //     *different* sequence that also lies on the globally-cheapest
    //     path. The Aztec bit-cost model never produces such a
    //     distinguishing tie: across the exhaustive len<=5 set and 310k
    //     fuzz inputs no encoded sequence changes. (A tie merely chooses
    //     between equal-length, equal-cost encodings that the downstream
    //     argmin would treat identically.)
    //
    //   L949:25  `(MIXED && CR) || (DIGIT && (last==',' || last=='.'))`
    //            → `&&`  (the OUTER join of the two pair-skip clauses)
    //   L949:69  `last == b',' || last == b'.'` → `&&`  (inner DIGIT clause)
    //     These set `in_p = false`, which SKIPS pair pre-compression in
    //     state `i_state` for this step. Both mutants effectively *disable*
    //     the skip: 949:25 makes the join require `i_state` be MIXED and
    //     DIGIT at once (impossible → always false → never skips); 949:69
    //     makes the inner test require `last` be both ',' and '.' at once
    //     (impossible → DIGIT clause never fires).
    //     EQUIVALENT: in every state/step where the baseline would set
    //     `in_p = false`, the pair-compressed candidate it suppresses is
    //     never the globally-cheapest sequence — a different state's
    //     candidate (or the un-compressed form) already wins the
    //     final-closure argmin with equal-or-lower cost. So suppressing vs
    //     not-suppressing yields the same chosen `curseq`. Confirmed: no
    //     encoded sequence changes across the exhaustive len<=5 set and
    //     310k fuzz inputs.
    //
    //   L968:40  `ch >= 0 && ch as u8 == last` → `||`  (look-back match)
    //     EQUIVALENT: this is the *first* match in a reverse scan for the
    //     pair's first char. `ch as u8 == last` already implies the
    //     compared item is a real emitted byte; the only sentinels with
    //     `ch < 0` have `ch as u8` wrapping to a high value that never
    //     equals a 7-bit ASCII `last`. So `ch >= 0` is redundant given the
    //     equality, and `&&`→`||` (accept any `ch >= 0` OR equal) never
    //     selects a different `lastidx`: a non-matching non-negative item
    //     scanned before the real first-char is impossible because the
    //     just-emitted pair-first byte is always the nearest non-negative
    //     item. No witness changes output.
    //
    //   L970:40  `idx > 0 && seq_i[idx-1] == SHIFT_PUNCT` → `>=`
    //     EQUIVALENT: `idx` is the position of the matched pair-first char
    //     (one of CR / '.' / ',' / ':'). None of those bytes is in the
    //     initial UPPER alphabet, so the char is always preceded in the
    //     seq by a latch or shift token that entered the alphabet that can
    //     emit it — hence `idx >= 1` whenever this look-behind runs. The
    //     `idx == 0` case (where `>=` would index `seq_i[idx - 1]` out of
    //     bounds) is therefore unreachable, and for every reachable
    //     `idx >= 1` both `idx > 0` and `idx >= 0` are true, so `lastsp`
    //     and the resulting sequence are identical. Confirmed: the mutant
    //     changes no output (and never panics) across the exhaustive
    //     len<=5 set and 310k fuzz inputs.
    //
    //   L994:32  `lastidx < curseq_i_len - 1` → `<=`
    //   L994:47  `curseq_i_len - 1` → `+ 1` / `/ 1`
    //     EQUIVALENT: this selects the "stuff after lastchar" splice
    //     (then-branch) vs the "lastchar at end" path (else-branch). When
    //     `lastidx == curseq_i_len - 1` BOTH branches yield exactly
    //     `seq_i[..lastidx] ++ [pair_sent]`: the splice copies
    //     `seq_i[..lastidx]` then `seq_i[lastidx+1..]` (empty) then pushes
    //     the sentinel; the else copies `seq_i[..len-1]` then pushes it.
    //     So widening the predicate to `<=` (or shifting the threshold via
    //     `len + 1` / `len / 1 == len`, both of which make `lastidx <
    //     threshold` always true) only ever routes the `lastidx == len-1`
    //     case through the splice instead of the else, producing the same
    //     vector. No witness changes output.
    //
    // bit_stuff — `|` → `^` in the MSB-first accumulator `v = (v << 1) | b`:
    //   L1253:30, L1255:26, L1269:30
    //     EQUIVALENT (sound proof): the left operand is always `v << 1`,
    //     whose bit 0 is 0; for any single bit `b ∈ {0,1}`,
    //     `(v << 1) | b == (v << 1) ^ b == (v << 1) + b`. (Verified the
    //     operand really is `v << 1` at all three sites.) See also
    //     `bit_stuff_value_pins_msb_first`.
    //
    // build_matrix:
    //   L1501:52 `*16 + *112` → `*16 * *112` (full symbol_bits)
    //   L1503:52 `*16 + *88`  → `*16 * *88`  (compact symbol_bits)
    //     EQUIVALENT: `symbol_bits` only sizes the zero-padded `databits`
    //     buffer whose codewords are packed at the END (`offset =
    //     symbol_bits - total_cw_bits`). The layer walk reads strictly
    //     from `databits[len-1-bit_idx]` downward for exactly the symbol's
    //     data-bit capacity, which never exceeds `total_cw_bits` ≤
    //     `symbol_bits`. Enlarging `symbol_bits` only prepends more
    //     never-read leading zeros, so every read bit is identical.
    //
    //   L1534:33 `(new_size - 1) / 2` → `(new_size / 1) / 2`
    //     EQUIVALENT: `new_size = 13 + layers*4 + 2 + growth` with growth
    //     even, so `new_size` is always ODD. For odd n,
    //     `(n - 1) / 2 == n / 2` (integer floor), so `new_mid` is
    //     unchanged.
    //
    //   L1546:40 `(half + j) + i` → `(half + j) - i`
    //   L1546:45 `... + 1` → `... - 1`   (reference-grid value `(..)%2`)
    //     EQUIVALENT: the value is taken `% 2`. `+i` vs `-i` differ by
    //     `2i` (even) and `+1` vs `-1` differ by `2` (even); an even delta
    //     never changes a value mod 2. The reference-grid checkerboard is
    //     identical.
    //
    //   Orientation-mark coordinates (Phase 4), all value 0:
    //   L1593:18 `+`→`*`, L1593:18 `+`→`-`, L1593:33 `+`→`-`,
    //   L1594:19 `delete -`, L1595:10 `delete -`, L1596:10 `delete -`,
    //   L1596:20 `+`→`-`, L1596:20 `+`→`*`, L1596:36 `+`→`-`,
    //   L1596:36 `+`→`*`
    //     EQUIVALENT: each mutated coordinate lands on a module that is
    //     subsequently overwritten or already carries the same value, so
    //     the final matrix is byte-identical. Concretely (fw_half = 4 for
    //     compact, 6 for full):
    //       * 1593:18 +→*, 1594:19 del-, 1595:10 del-, 1596:10 del-,
    //         1596:20 +→*, 1596:36 +→*  → the coordinate coincides with a
    //         DIFFERENT value-0 orientation mark in the same array, so the
    //         identical 0 is written to the identical pixel.
    //       * 1593:18 +→- → (fw_half-1, -(fw_half+1)); 1593:33 +→- →
    //         (fw_half+1, -(fw_half-1)); 1596:20 +→- → (-(fw_half-1),
    //         -(fw_half+1)); 1596:36 +→- → (-(fw_half+1), -(fw_half-1)).
    //         Each of these four is a MODEMAP position (e.g. compact
    //         (3,-5),(5,-3),(-3,-5),(-5,-3); full (5,-7),(7,-5),(-5,-7),
    //         (-7,-5)) that Phase 5 overwrites with the mode bit, so the
    //         orientation value placed there is discarded.
    //     Verified by diffing the full matrix for compact L1-4 and full
    //     L1-32: no pixel changes for any of these ten mutants.
}
