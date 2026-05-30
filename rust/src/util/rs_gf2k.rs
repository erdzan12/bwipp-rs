//! Generic Reed-Solomon over GF(2^k) for `k` ∈ {4, 6, 8, 10, 12}.
//!
//! Aztec Code uses different Galois fields depending on layer count:
//!
//! | bits/cw | field size | primitive poly |
//! |---------|------------|----------------|
//! | 4       | 16         | 19   (`x^4 + x + 1`)               |
//! | 6       | 64         | 67   (`x^6 + x + 1`)               |
//! | 8       | 256        | 301  (`x^8 + x^4 + x^3 + x^2 + 1`) |
//! | 10      | 1024       | 1033 (`x^10 + x^3 + 1`)            |
//! | 12      | 4096       | 4201 (`x^12 + x^6 + x^5 + x^3 + 1`)|
//!
//! The polynomial values are stored on each [`GfParams`] instance
//! so callers pick the right field per symbol size at runtime.

#![allow(dead_code)]

/// Parameters describing a `GF(2^k)` arithmetic context: the order
/// `q = 2^k` and the primitive polynomial used for reduction.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GfParams {
    /// Order of the field (`2^k`).
    pub size: u32,
    /// Primitive polynomial as a binary integer (degree `k`).
    pub poly: u32,
}

impl GfParams {
    /// `k` bits per codeword.
    pub fn k(&self) -> u32 {
        self.size.trailing_zeros()
    }
}

/// GF(16) parameters — Aztec doesn't actually use 4-bit codewords
/// in published metrics, but BWIPP exposes the entry. Included for
/// completeness.
pub(crate) const GF16: GfParams = GfParams { size: 16, poly: 19 };
/// GF(64) parameters (BWIPP's `azteccode_rsparams[6]`).
pub(crate) const GF64: GfParams = GfParams { size: 64, poly: 67 };
/// GF(256) parameters (BWIPP's `azteccode_rsparams[8]`).
pub(crate) const GF256: GfParams = GfParams {
    size: 256,
    poly: 301,
};
/// GF(1024) parameters (BWIPP's `azteccode_rsparams[10]`).
pub(crate) const GF1024: GfParams = GfParams {
    size: 1024,
    poly: 1033,
};
/// GF(4096) parameters (BWIPP's `azteccode_rsparams[12]`).
pub(crate) const GF4096: GfParams = GfParams {
    size: 4096,
    poly: 4201,
};

/// Look up the GF parameters for a given `bps` (bits per codeword)
/// — Aztec metrics-driven dispatch.
pub(crate) fn gf_for_bps(bps: u8) -> Option<GfParams> {
    match bps {
        4 => Some(GF16),
        6 => Some(GF64),
        8 => Some(GF256),
        10 => Some(GF1024),
        12 => Some(GF4096),
        _ => None,
    }
}

/// Multiply two elements of `GF(2^k)` using shift-and-XOR with the
/// supplied primitive polynomial for reduction.
pub(crate) fn multiply(a: u32, b: u32, gf: GfParams) -> u32 {
    let mask = gf.size - 1; // (1 << k) - 1
    let high_bit = gf.size; // 1 << k
    debug_assert!(a <= mask);
    debug_assert!(b <= mask);
    let mut prod: u32 = 0;
    let mut a = a;
    let mut b = b;
    while b != 0 {
        if b & 1 != 0 {
            prod ^= a;
        }
        a <<= 1;
        if a & high_bit != 0 {
            a ^= gf.poly;
        }
        b >>= 1;
    }
    prod & mask
}

/// Build the degree-`k` generator polynomial for `k`-symbol Reed-
/// Solomon over `gf`. Returns `k + 1` coefficients in
/// increasing-degree order: `[g_0, g_1, …, g_{k-1}, g_k = 1]`.
///
/// Construction mirrors BWIPP's iterative product of `(x + α^i)`
/// for `i = 1..=k`, where `α` is the primitive element (= 2 in
/// our polynomial-basis representation).
pub(crate) fn generator_poly(k: usize, gf: GfParams) -> Vec<u32> {
    let mask = gf.size - 1;
    let mut poly = vec![0u32; k + 1];
    poly[0] = 1;
    let mut alpha: u32 = 1;
    for _root in 1..=k {
        alpha <<= 1;
        if alpha & gf.size != 0 {
            alpha ^= gf.poly;
        }
        alpha &= mask;
        for j in (1..=k).rev() {
            poly[j] = poly[j - 1] ^ multiply(poly[j], alpha, gf);
        }
        poly[0] = multiply(poly[0], alpha, gf);
    }
    poly
}

/// Compute `k` Reed-Solomon check codewords over `gf` for `data`.
/// Returns the `k` check codewords; the natural orientation is
/// BWIPP's `rscodes[0..k]` slot order — caller may need to reverse
/// for symbology-specific output convention.
pub(crate) fn encode_k(data: &[u32], k: usize, gf: GfParams) -> Vec<u32> {
    assert!(k > 0);
    let gen = generator_poly(k, gf);
    let n = data.len();
    let mut codes = vec![0u32; n + k];
    for (i, &d) in data.iter().enumerate() {
        codes[n + k - 1 - i] = d;
    }
    for pos in (0..=codes.len() - k - 1).rev() {
        let pivot = codes[pos + k];
        for j in 0..=k {
            codes[pos + j] ^= multiply(gen[j], pivot, gf);
        }
    }
    codes[..k].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gf_for_bps_dispatch() {
        assert_eq!(gf_for_bps(6).unwrap().size, 64);
        assert_eq!(gf_for_bps(6).unwrap().poly, 67);
        assert_eq!(gf_for_bps(8).unwrap().size, 256);
        assert_eq!(gf_for_bps(10).unwrap().size, 1024);
        assert_eq!(gf_for_bps(12).unwrap().size, 4096);
        assert!(gf_for_bps(7).is_none());
    }

    /// Stage 11.A8c — strengthen `gf_for_bps` arm coverage. The
    /// existing test asserted `size` for bps=6/8/10/12 but skipped
    /// bps=4 (GF16) and only checked `poly` for bps=6 — leaving arm-
    /// swap mutations on GF16↔GF1024 (both `poly` differ but `size`
    /// stays distinct only via the assert) and missing-poly-pin
    /// mutations free to slip.
    ///
    /// Mutations to catch:
    ///   - `4 => Some(GF16)` deleted: bps=4 silently returns None and
    ///     the Aztec L1 (4-bit codeword) path errors at runtime, but
    ///     no existing test calls bps=4 directly.
    ///   - swap of any two GF parameter consts (e.g. GF1024↔GF4096):
    ///     the unique-poly anchor catches this.
    ///   - replacement of the `None` catch-all with `Some(GF16)`: all
    ///     four invalid-bps assertions catch this.
    #[test]
    fn gf_for_bps_pins_every_arm_including_bps4() {
        // bps=4 → GF16 (Aztec L1 compact uses 4-bit codewords).
        let g4 = gf_for_bps(4).expect("bps=4 must dispatch to GF16");
        assert_eq!(g4.size, 16, "GF16 size");
        assert_eq!(g4.poly, 19, "GF16 poly x^4+x+1 = 0b10011 = 19");
        // bps=8/10/12: pin poly values (previously only size pinned).
        assert_eq!(gf_for_bps(8).unwrap().poly, 301, "GF256 BWIPP poly = 301");
        assert_eq!(
            gf_for_bps(10).unwrap().poly,
            1033,
            "GF1024 BWIPP poly = 1033"
        );
        assert_eq!(
            gf_for_bps(12).unwrap().poly,
            4201,
            "GF4096 BWIPP poly = 4201"
        );
        // Catch-all: bps values outside {4,6,8,10,12} → None.
        assert!(gf_for_bps(0).is_none(), "bps=0 → None");
        assert!(gf_for_bps(5).is_none(), "bps=5 between valid arms → None");
        assert!(gf_for_bps(9).is_none(), "bps=9 between valid arms → None");
        assert!(gf_for_bps(11).is_none(), "bps=11 between valid arms → None");
        assert!(gf_for_bps(16).is_none(), "bps=16 above valid arms → None");
    }

    #[test]
    fn multiply_gf64_matches_existing_rs_gf64() {
        // Spot-check that our generic multiply agrees with the
        // legacy crate::util::rs_gf64 for GF(64) inputs.
        for a in 0u32..64 {
            for b in 0u32..64 {
                let want = u32::from(crate::util::rs_gf64::multiply(a as u8, b as u8));
                let got = multiply(a, b, GF64);
                assert_eq!(got, want, "GF64({a} × {b}) mismatch");
            }
        }
    }

    #[test]
    fn generator_poly_gf64_k4_matches_auspost() {
        // AusPost generator polynomial: [48, 17, 29, 30, 1].
        let g = generator_poly(4, GF64);
        assert_eq!(g, vec![48, 17, 29, 30, 1]);
    }

    #[test]
    fn generator_poly_gf256_k4() {
        // QR Code generator polynomial for 4 check codewords over
        // GF(256) with primitive 285 differs from GF(256)/301 used
        // here; this just spot-checks our construction self-consistency.
        let g = generator_poly(4, GF256);
        // First coefficient (constant term) of the polynomial
        // (x + α)(x + α²)(x + α³)(x + α⁴) over GF(256) with poly 301.
        // α = 2, α² = 4, α³ = 8, α⁴ = 16. Product = 2*4*8*16 = 1024,
        // reduced mod 256 via poly 301... we just verify shape here.
        assert_eq!(g.len(), 5);
        assert_eq!(g[4], 1); // leading coefficient is always 1.
    }

    #[test]
    fn encode_k_round_trips_against_legacy_rs_gf64() {
        // AusPost data with k=4 over GF(64) — same fixture the legacy
        // module verifies against bwip-js.
        let data = [4u8, 17, 9, 5, 26, 9, 43];
        let data_u32: Vec<u32> = data.iter().map(|&v| u32::from(v)).collect();
        let got = encode_k(&data_u32, 4, GF64);
        let want = crate::util::rs_gf64::encode_4(&data);
        let want_u32: Vec<u32> = want.iter().map(|&v| u32::from(v)).collect();
        assert_eq!(got, want_u32);
    }

    /// Stage 11.A8c — pin `gf_for_bps` lookup at every supported width
    /// + rejection of unsupported widths. Kills `delete match arm`
    /// mutations on lines 60-66.
    #[test]
    fn gf_for_bps_supported_widths() {
        // Supported widths each return Some with matching size 2^bps.
        let cases: &[(u8, u32)] = &[(4, 16), (6, 64), (8, 256), (10, 1024), (12, 4096)];
        for &(bps, expected_size) in cases {
            let gf = gf_for_bps(bps).unwrap_or_else(|| panic!("gf_for_bps({bps}) returned None"));
            assert_eq!(gf.size, expected_size, "bps={bps} size");
        }
        // Unsupported widths → None.
        for bps in [0u8, 1, 3, 5, 7, 9, 11, 13, 255] {
            assert!(gf_for_bps(bps).is_none(), "bps={bps} should be None");
        }
    }

    /// Stage 11.A8c — pin `multiply` GF(2^k) parameterised by field
    /// (GF16 / GF256). Kills `<<= with >>=` / `^= with &=` /
    /// `& mask` / `& high_bit` mutations on lines 82-90.
    #[test]
    fn multiply_gf2k_identity_and_zero() {
        for gf in [GF16, GF64, GF256] {
            // Zero absorption.
            assert_eq!(multiply(0, 1, gf), 0);
            assert_eq!(multiply(1, 0, gf), 0);
            assert_eq!(multiply(0, 0, gf), 0);
            // Identity.
            assert_eq!(multiply(1, 1, gf), 1);
            let max = gf.size - 1;
            assert_eq!(multiply(1, max, gf), max);
            assert_eq!(multiply(max, 1, gf), max);
            // Commutativity.
            assert_eq!(multiply(3, 5, gf), multiply(5, 3, gf));
            assert_eq!(multiply(13, 41 & max, gf), multiply(41 & max, 13, gf));
        }
    }
}
