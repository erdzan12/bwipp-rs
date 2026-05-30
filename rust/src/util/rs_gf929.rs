//! Reed-Solomon error correction over GF(929) = ℤ/929ℤ.
//!
//! Used by PDF417 and Micro PDF417 to compute the error-correction
//! codewords appended to every symbol. Unlike GF(64) and GF(113), the
//! field GF(929) is a prime field — there is no polynomial reduction
//! step, so multiplication is just `(a * b) mod 929` and the primitive
//! root used by BWIPP is α = 3 (so `α^928 ≡ 1`).
//!
//! The generator polynomial is the product `(x - α^1)(x - α^2)…(x - α^k)`
//! and is built by repeated polynomial multiplication; the encoder is
//! a textbook systematic LFSR pass that mirrors BWIPP's `bwipp_rsecprime`.
//! Verified against bwip-js's PDF417 output at eclevel=2 (k=8) and
//! eclevel=3 (k=16).
//!
//! Used by [`crate::symbology::pdf417`], [`crate::symbology::micropdf417`]
//! (which share this RS encoder), and [`crate::symbology::composite`]
//! for the GS1 Composite-Component-A check codewords.

/// Multiply two field elements in GF(929). Inline-equivalent to
/// `(a * b) % 929`, broken out so callers and tests can use a single
/// named operation.
#[inline]
fn multiply(a: u16, b: u16) -> u16 {
    ((a as u32 * b as u32) % 929) as u16
}

/// The degree-`k` generator polynomial for `k`-symbol Reed-Solomon over
/// GF(929), computed as `∏_{i=1..=k} (x - α^i)` where α = 3. Returned
/// as `k+1` coefficients in increasing-degree order — `[g0, g1, …, gk=1]`
/// — matching BWIPP's `$_.coeffs` layout including the leading-term
/// `gk = 1`.
pub(crate) fn generator_poly(k: usize) -> Vec<u16> {
    let mut coeffs = vec![0u16; k + 1];
    coeffs[0] = 1;
    let mut alpha_pow: u16 = 1;
    for j in 1..=k {
        alpha_pow = multiply(alpha_pow, 3);
        // Shift: coeffs[j] = coeffs[j-1].
        coeffs[j] = coeffs[j - 1];
        // Walk c down so we read coeffs[c-1] before it's overwritten.
        for c in (1..j).rev() {
            let v = multiply(coeffs[c], alpha_pow);
            let diff = (929 + coeffs[c - 1] as u32 - v as u32) % 929;
            coeffs[c] = diff as u16;
        }
        // coeffs[0] = (-coeffs[0] * α^j) mod 929.
        let v = multiply(coeffs[0], alpha_pow);
        coeffs[0] = ((929 - v as u32) % 929) as u16;
    }
    coeffs
}

/// Compute `k` Reed-Solomon check codewords for `data` over GF(929).
/// Mirrors BWIPP's `bwipp_rsecprime` LFSR loop exactly: starts with an
/// all-zero LFSR of length `k`, shifts each data symbol's feedback
/// through it via `coeffs[nc-l-1..0]`. Returned slice is `[lfsr[0], …,
/// lfsr[k-1]]` in BWIPP's natural order (these become `cws[n..n+k]`).
pub(crate) fn encode(data: &[u16], k: usize) -> Vec<u16> {
    let coeffs = generator_poly(k);
    let mut lfsr = vec![0u16; k];
    for &d in data {
        let feedback = ((929 + d as u32 - lfsr[0] as u32) % 929) as u16;
        // Shift left: lfsr[l] ← lfsr[l+1] + coeffs[k - l - 1] * feedback.
        for l in 0..(k - 1) {
            let v = multiply(coeffs[k - l - 1], feedback) as u32;
            lfsr[l] = ((lfsr[l + 1] as u32 + v) % 929) as u16;
        }
        // lfsr[k-1] ← coeffs[0] * feedback.
        lfsr[k - 1] = multiply(coeffs[0], feedback);
    }
    lfsr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiply_basic() {
        assert_eq!(multiply(0, 5), 0);
        assert_eq!(multiply(5, 0), 0);
        assert_eq!(multiply(1, 928), 928);
        assert_eq!(multiply(3, 3), 9);
        // 928 * 928 mod 929 should be 1 (since 928 ≡ -1, so (-1)^2 = 1).
        assert_eq!(multiply(928, 928), 1);
    }

    /// Generator polynomial for k=8 (PDF417 eclevel=2). Captured live
    /// from bwip-js's `$_.coeffs` initializer.
    #[test]
    fn generator_poly_k8_matches_bwip_js() {
        let g = generator_poly(8);
        assert_eq!(g, [237, 308, 436, 284, 646, 653, 428, 379, 1]);
    }

    /// LFSR encoding for the PDF417 "PDF417" eclevel=2 vector. Captured
    /// via the patched `tools/oracle-pdf417.js` debugecc dump.
    #[test]
    fn encode_pdf417_canonical_vector() {
        let data = [6u16, 453, 178, 121, 239, 900];
        let check = encode(&data, 8);
        assert_eq!(check, [466, 823, 655, 326, 814, 259, 528, 611]);
    }

    /// Wider vector: 24-codeword data, k=16 (eclevel=3) for an URL payload.
    #[test]
    fn encode_pdf417_url_vector_k16() {
        let data = [
            24u16, 817, 589, 468, 854, 589, 816, 259, 230, 59, 512, 432, 889, 137, 115, 13, 841,
            79, 811, 668, 465, 886, 528, 900,
        ];
        let check = encode(&data, 16);
        assert_eq!(
            check,
            [506, 21, 681, 617, 210, 844, 772, 529, 305, 209, 43, 171, 133, 55, 229, 162]
        );
    }

    /// Stage 11.A8c — pin `multiply` GF(929) modular reduction at
    /// boundary values. Kills `% 929` / `* with /` / `+ with *`
    /// mutations on line 24.
    #[test]
    fn multiply_gf929_boundaries() {
        // Identity: 1 * x = x.
        assert_eq!(multiply(1, 5), 5);
        assert_eq!(multiply(7, 1), 7);
        // Zero: 0 * x = 0.
        assert_eq!(multiply(0, 100), 0);
        assert_eq!(multiply(100, 0), 0);
        // Small product < 929.
        assert_eq!(multiply(5, 7), 35);
        // Product that exceeds 929: 100 * 100 = 10000. 10000 % 929
        // = 10000 - 10*929 = 10000 - 9290 = 710.
        assert_eq!(multiply(100, 100), 710);
        // Boundary: 928 * 928 = 861184. 861184 % 929 = ?
        // 861184 / 929 ≈ 927.something. 927 * 929 = 861183. So remainder 1.
        assert_eq!(multiply(928, 928), 1);
        // α^2 = 9 (α = 3), so 3 * 3 = 9.
        assert_eq!(multiply(3, 3), 9);
        // α^3 = 27.
        assert_eq!(multiply(9, 3), 27);
    }

    /// Stage 11.A8c — pin `generator_poly` shape and leading term.
    /// Kills function-replacement mutants and `k + 1` arithmetic on
    /// line 33.
    #[test]
    fn generator_poly_shape() {
        // k=0 → just [1] (the leading 1 with no other roots).
        let g0 = generator_poly(0);
        assert_eq!(g0, vec![1]);

        // k=1 → degree-1 polynomial (x - α): coeffs = [-α mod 929, 1]
        // = [929-3, 1] = [926, 1]. Verifies the leading 1 at index k.
        let g1 = generator_poly(1);
        assert_eq!(g1.len(), 2);
        assert_eq!(g1[1], 1, "leading coeff must be 1");
        assert_eq!(g1[0], 926, "const term = -α mod 929");

        // k=2 → length 3, leading 1.
        let g2 = generator_poly(2);
        assert_eq!(g2.len(), 3);
        assert_eq!(g2[2], 1);

        // Larger k still has the leading 1.
        let g8 = generator_poly(8);
        assert_eq!(g8.len(), 9);
        assert_eq!(g8[8], 1);
    }
}
