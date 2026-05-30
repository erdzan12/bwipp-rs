//! Reed-Solomon error correction over GF(64) = GF(2^6).
//!
//! Used by Australia Post 4-state customer barcodes (`auscustomer`,
//! `ausreplypaid`, `ausrouting`, `ausredirection`) to compute the 4
//! check codewords appended to every symbol.
//!
//! The field arithmetic uses the primitive polynomial
//! `x^6 + x + 1` (binary `1000011` = 67 decimal). The carry rule when
//! a multiplication overflows bit 6 is `value ^= 67`. Tables and
//! generator polynomial match bwip-js's `bwipp_auspost` initialization
//! block (line 16462 onwards in the 2026-03-31 vendor snapshot).
//!
//! This module is intentionally `pub(crate)` — symbology modules
//! import it directly, but the field arithmetic is not part of the
//! public crate API.

/// Multiply two elements of GF(64). Uses the polynomial representation
/// (each element is a 6-bit value, treated as a polynomial of degree ≤ 5
/// in GF(2)). Result is reduced modulo `x^6 + x + 1`.
pub(crate) fn multiply(a: u8, b: u8) -> u8 {
    debug_assert!(a < 64);
    debug_assert!(b < 64);
    let mut prod: u16 = 0;
    let mut a16 = a as u16;
    let mut b16 = b as u16;
    while b16 != 0 {
        if b16 & 1 != 0 {
            prod ^= a16;
        }
        a16 <<= 1;
        if a16 & 0x40 != 0 {
            a16 ^= 0x43; // x^6 + x + 1
        }
        b16 >>= 1;
    }
    (prod & 0x3f) as u8
}

/// The degree-4 generator polynomial for 4-symbol Reed-Solomon over
/// GF(64), computed iteratively as the product of `(x + alpha^i)` for
/// `i = 1..=4`. Returned as 5 coefficients in increasing-degree order:
/// `[g0, g1, g2, g3, g4=1]`. Mirrors bwip-js's `$_.rspoly` initialization.
pub(crate) fn generator_poly_4() -> [u8; 5] {
    let mut poly: [u8; 5] = [1, 0, 0, 0, 0];
    let mut alpha: u8 = 1; // alpha^0
    for _root in 1..=4 {
        // alpha ← alpha << 1 in GF(2), with the GF(64) reduction.
        alpha <<= 1;
        if alpha & 0x40 != 0 {
            alpha ^= 0x43;
        }
        // Multiply poly by (x + alpha). In coefficient form:
        // new[i] = old[i-1] ^ (old[i] * alpha).
        // BWIPP walks high-to-low to avoid clobbering source values.
        for j in (1..=4_usize).rev() {
            poly[j] = poly[j - 1] ^ multiply(poly[j], alpha);
        }
        poly[0] = multiply(poly[0], alpha);
    }
    poly
}

/// Compute the 4 Reed-Solomon check codewords for an AusPost data
/// stream. `data` carries the input codewords (each `< 64`); the
/// returned 4-element array is the checks `[k0, k1, k2, k3]` such that
/// `[k0, k1, k2, k3, data[0], data[1], …]` viewed as a polynomial is
/// divisible by the generator.
///
/// The bwip-js implementation operates on a single `rscodes` array with
/// the 4 check slots at the *low* end and the data in *reverse* order
/// at the high end. We accept the data in natural (forward) order and
/// return the four check codewords in the same orientation BWIPP uses
/// when it writes them back into `encstr` — `[k3, k2, k1, k0]` from
/// bwip-js's `rscodes[0..4]`.
pub(crate) fn encode_4(data: &[u8]) -> [u8; 4] {
    let gen = generator_poly_4();
    // BWIPP layout: rscodes has length data.len() + 4; first 4 are
    // check slots (init 0); the rest hold data in REVERSE order.
    let n = data.len();
    let mut codes = vec![0u8; n + 4];
    for (i, &d) in data.iter().enumerate() {
        codes[n + 4 - 1 - i] = d;
    }
    // For each position pos from (codes.len()-5) down to 0:
    //   for j in 0..=4: codes[pos+j] ^= gen[j] * codes[pos+4].
    for pos in (0..=codes.len() - 5).rev() {
        let pivot = codes[pos + 4];
        for j in 0..=4 {
            codes[pos + j] ^= multiply(gen[j], pivot);
        }
    }
    [codes[0], codes[1], codes[2], codes[3]]
}

/// General degree-`k` generator polynomial for `k`-symbol Reed-Solomon
/// over GF(64). Returns `k + 1` coefficients in increasing-degree
/// order: `[g_0, g_1, …, g_{k-1}, g_k = 1]`.
///
/// Same construction as [`generator_poly_4`] but with the loop bound
/// generalised. Used by MaxiCode (k = 10 for the primary check,
/// k = 10 or 20 for the secondary check depending on mode).
pub(crate) fn generator_poly(k: usize) -> Vec<u8> {
    let mut poly = vec![0u8; k + 1];
    poly[0] = 1;
    let mut alpha: u8 = 1;
    for _root in 1..=k {
        alpha <<= 1;
        if alpha & 0x40 != 0 {
            alpha ^= 0x43;
        }
        for j in (1..=k).rev() {
            poly[j] = poly[j - 1] ^ multiply(poly[j], alpha);
        }
        poly[0] = multiply(poly[0], alpha);
    }
    poly
}

/// Compute `k` Reed-Solomon check codewords over GF(64) for `data`.
/// Generalisation of [`encode_4`]. The returned slice's element `[0]`
/// is the BWIPP `rscodes[0]` slot, `[k-1]` is `rscodes[k-1]`. Used
/// by MaxiCode's primary and secondary RS stages.
pub(crate) fn encode_k(data: &[u8], k: usize) -> Vec<u8> {
    assert!(k > 0, "k must be positive");
    let gen = generator_poly(k);
    let n = data.len();
    let mut codes = vec![0u8; n + k];
    for (i, &d) in data.iter().enumerate() {
        codes[n + k - 1 - i] = d;
    }
    for pos in (0..=codes.len() - k - 1).rev() {
        let pivot = codes[pos + k];
        for j in 0..=k {
            codes[pos + j] ^= multiply(gen[j], pivot);
        }
    }
    codes[..k].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiply_known_values() {
        // alpha^0 = 1, alpha^1 = 2 in our representation.
        assert_eq!(multiply(1, 1), 1);
        assert_eq!(multiply(2, 1), 2);
        assert_eq!(multiply(2, 2), 4);
        assert_eq!(multiply(2, 32), multiply(32, 2));
        // alpha * alpha^5 = alpha^6 = alpha + 1 = 3 (since x^6 = x + 1).
        assert_eq!(multiply(2, 32), 3);
        // Identity: 0 * x = 0.
        assert_eq!(multiply(0, 17), 0);
        assert_eq!(multiply(17, 0), 0);
    }

    #[test]
    fn multiply_is_commutative() {
        for a in 0u8..64 {
            for b in 0u8..64 {
                assert_eq!(
                    multiply(a, b),
                    multiply(b, a),
                    "comm failed for a={a} b={b}"
                );
            }
        }
    }

    /// The AusPost generator polynomial computed by bwip-js for 4 check
    /// codewords. Captured by reading `$_.rspoly` after the constructor
    /// block in bwipp_auspost (the values are stable; they only depend
    /// on the primitive polynomial and the check count).
    #[test]
    fn generator_poly_matches_bwip_js() {
        let g = generator_poly_4();
        // Coefficients [g0, g1, g2, g3, g4=1] for the polynomial
        // (x + α)(x + α²)(x + α³)(x + α⁴) over GF(64). Captured live
        // from bwip-js's `$_.rspoly` initializer.
        assert_eq!(g, [48, 17, 29, 30, 1]);
    }

    /// Golden RS check for "1112345678" (FCC=11, DPID=12345678) captured
    /// from bwip-js's auspost encoder: rscodes[0..4] = [15, 19, 44, 26].
    /// The data is presented in the natural forward order; `encode_4`
    /// internally reverses to match BWIPP's storage convention.
    #[test]
    fn auspost_check_matches_bwip_js() {
        // Data codewords for "1112345678" derived from the 21-cell data
        // section of encstr, in groups of 3 forming base-4 values. BWIPP
        // stores these reversed in rscodes[4..11] = [43, 9, 26, 5, 9, 17, 4];
        // we pass them forward.
        let data = [4u8, 17, 9, 5, 26, 9, 43];
        let check = encode_4(&data);
        assert_eq!(check, [15, 19, 44, 26]);
    }

    #[test]
    fn generator_poly_for_k_matches_bwip_js() {
        // k=4 should match the AusPost generator (existing constant).
        assert_eq!(generator_poly(4), vec![48, 17, 29, 30, 1]);
        // k=10 — MaxiCode primary RS check (10 codewords). Captured
        // by running BWIPP's construction algorithm directly.
        assert_eq!(
            generator_poly(10),
            vec![46, 44, 49, 3, 2, 57, 42, 39, 28, 31, 1],
        );
        // k=20 — MaxiCode secondary RS check for mode 2/3/4 (and the
        // halves of mode 5's interleaved secondary).
        assert_eq!(
            generator_poly(20),
            vec![59, 23, 19, 31, 33, 38, 17, 22, 48, 15, 36, 57, 37, 22, 8, 27, 33, 11, 44, 23, 1],
        );
    }

    #[test]
    fn encode_k_round_trips_with_encode_4() {
        // encode_k(_, 4) should match encode_4 for any input.
        let data = [4u8, 17, 9, 5, 26, 9, 43];
        let check_4 = encode_4(&data);
        let check_k = encode_k(&data, 4);
        assert_eq!(check_k, check_4.to_vec());
    }

    /// Stage 11.A8c — pin `multiply` GF(64) operations at identity
    /// and the polynomial reduction. Kills `^= with &=` / `<< with >>`
    /// / `0x43 reduction polynomial` mutations on lines 23-36.
    #[test]
    fn multiply_gf64_identity_and_zero() {
        // Identity: 1 * x = x for x in [0, 63].
        assert_eq!(multiply(1, 0), 0);
        assert_eq!(multiply(1, 5), 5);
        assert_eq!(multiply(1, 63), 63);
        assert_eq!(multiply(5, 1), 5);
        // Zero absorption.
        assert_eq!(multiply(0, 0), 0);
        assert_eq!(multiply(0, 63), 0);
        assert_eq!(multiply(63, 0), 0);
        // Self-multiplication: 1*1=1, 2*2 = 4 (no overflow in GF(64)).
        assert_eq!(multiply(1, 1), 1);
        assert_eq!(multiply(2, 2), 4);
        assert_eq!(multiply(4, 4), 16);
        assert_eq!(multiply(8, 8), 0x40 ^ 0x43); // = 0x03 = 3, after reduction.
                                                 // Commutativity check (catches asymmetric mutations).
        assert_eq!(multiply(5, 7), multiply(7, 5));
        assert_eq!(multiply(13, 41), multiply(41, 13));
    }

    /// Stage 11.A8c — pin `generator_poly_4` against `generator_poly(4)`
    /// equivalence. Kills function-replacement mutants on lines 43
    /// or 102.
    #[test]
    fn generator_poly_4_matches_general_form() {
        let g4 = generator_poly_4();
        let gk4 = generator_poly(4);
        // Both must produce the same 5-element polynomial.
        assert_eq!(g4.len(), 5);
        assert_eq!(gk4.len(), 5);
        for i in 0..5 {
            assert_eq!(g4[i], gk4[i], "coeff {i}: g4={} vs gk4={}", g4[i], gk4[i]);
        }
        // Leading term is 1.
        assert_eq!(g4[4], 1);
        // Length-1 base case: k=0 → [1].
        assert_eq!(generator_poly(0), vec![1]);
    }
}
