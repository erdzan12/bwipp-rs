//! Reed-Solomon error correction over the prime field `GF(113)`.
//!
//! DotCode is the only BWIPP symbology that uses GF(113). The field is a
//! prime field with **3 as a primitive root**, so all non-zero elements can
//! be expressed as a power of 3 (mod 113). BWIPP exposes a `rsalog` table
//! seeded by `1, 3, 9, 27, ...` and uses it to compute the generator
//! polynomial and the check codewords for DotCode messages.
//!
//! This module reproduces that arithmetic in safe Rust. The public surface
//! is small on purpose — only [`generator_poly`] and [`encode`].
//!
//! Reference: bwip-js `bwipp_dotcode` builds `rsalog` and applies the same
//! polynomial multiplication / mod-113 reduction we use here.

// DotCode isn't wired into the encoder yet (it needs a dot renderer and the
// big charset/mask code), so the helpers in this module are temporarily
// unused outside the unit tests. Allow dead code here rather than `pub use`
// them prematurely — once DotCode lands, the allow can be removed.
#![allow(dead_code)]

/// The DotCode prime field modulus.
pub const P: u32 = 113;

/// Number of non-zero elements in `GF(P)`; equivalently the order of the
/// multiplicative group.
pub const ORDER: usize = (P - 1) as usize;

/// Build the `log` and `antilog` tables for `GF(P)` with primitive root 3.
///
/// `antilog[i] = 3^i mod 113` for `i in 0..ORDER`, and `log[a]` is the unique
/// `i` such that `antilog[i] == a` (for `a != 0`).
pub fn build_tables() -> ([u8; ORDER], [u8; P as usize]) {
    let mut antilog = [0u8; ORDER];
    let mut log = [0u8; P as usize];
    let mut acc: u32 = 1;
    for (i, slot) in antilog.iter_mut().enumerate() {
        *slot = acc as u8;
        log[acc as usize] = i as u8;
        acc = (acc * 3) % P;
    }
    (antilog, log)
}

/// Compute the generator polynomial of degree `nck` for Reed-Solomon over
/// `GF(P)`. The polynomial is `Π (x - 3^i)` for `i = 1..=nck`. Returned
/// with the **leading (highest-degree) coefficient first**, so `out[0] = 1`
/// and `out[nck]` is the constant term.
pub fn generator_poly(nck: usize) -> Vec<u32> {
    let (antilog, _log) = build_tables();
    // Internal representation during the loop stores lowest degree first
    // because that's the more convenient direction for polynomial
    // multiplication. We reverse before returning.
    let mut g: Vec<u32> = vec![1];
    for i in 1..=nck {
        let root = antilog[i % ORDER] as u32;
        let neg_root = (P - root) % P;
        let mut new_g = vec![0u32; g.len() + 1];
        for (j, &coef) in g.iter().enumerate() {
            new_g[j + 1] = (new_g[j + 1] + coef) % P;
            new_g[j] = (new_g[j] + coef * neg_root) % P;
        }
        g = new_g;
    }
    g.reverse();
    g
}

/// Encode a Reed-Solomon message over `GF(P)`.
///
/// `data` is the message codewords (each in `0..P`). `nck` is the number of
/// check codewords to append. Returns `data ++ check`, length `data.len() +
/// nck`.
///
/// Panics if any input codeword is out of range (`>= P`).
pub fn encode(data: &[u32], nck: usize) -> Vec<u32> {
    assert!(data.iter().all(|&d| d < P), "data codeword >= {P}");
    let gen = generator_poly(nck);

    // Long-division: keep a working register of length `nck`, walk the data
    // codewords, and for each one xor-equivalent (add mod P here) the
    // shifted generator into the register.
    let mut register = vec![0u32; nck];
    for &d in data {
        let leading = (d + register[0]) % P;
        // Shift register left by one.
        for i in 0..nck - 1 {
            register[i] = register[i + 1];
        }
        register[nck - 1] = 0;
        if leading != 0 {
            for i in 0..nck {
                let term = (leading * gen[i + 1]) % P;
                register[i] = (register[i] + (P - term) % P) % P;
            }
        }
    }

    let mut out = data.to_vec();
    out.extend_from_slice(&register);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn antilog_first_entries_match_bwipp_seed() {
        // BWIPP's loop seeds with 1 and multiplies by 3 each step:
        //   1, 3, 9, 27, 81, (81*3=243 mod 113=17), (17*3=51), ...
        let (antilog, _) = build_tables();
        assert_eq!(antilog[0], 1);
        assert_eq!(antilog[1], 3);
        assert_eq!(antilog[2], 9);
        assert_eq!(antilog[3], 27);
        assert_eq!(antilog[4], 81);
        assert_eq!(antilog[5], (81 * 3) % 113);
        // Full cycle length must be P - 1 (otherwise 3 isn't a primitive root).
        let mut seen = std::collections::HashSet::new();
        for &v in &antilog {
            assert!(seen.insert(v), "duplicate in antilog: {v}");
        }
        assert_eq!(seen.len(), ORDER);
    }

    #[test]
    fn log_is_inverse_of_antilog() {
        let (antilog, log) = build_tables();
        for (i, &a) in antilog.iter().enumerate() {
            assert_eq!(log[a as usize] as usize, i);
        }
    }

    #[test]
    fn generator_poly_degree_matches_nck() {
        for nck in 1..8 {
            let g = generator_poly(nck);
            assert_eq!(g.len(), nck + 1);
            assert_eq!(g[0], 1);
        }
    }

    #[test]
    fn encode_appends_nck_codewords() {
        let data = [10u32, 20, 30, 40, 50];
        let nck = 4;
        let out = encode(&data, nck);
        assert_eq!(out.len(), data.len() + nck);
        assert_eq!(&out[..data.len()], &data);
    }

    #[test]
    fn encode_is_deterministic() {
        let data = [1u32, 2, 3, 4, 5];
        let nck = 5;
        let a = encode(&data, nck);
        let b = encode(&data, nck);
        assert_eq!(a, b);
    }

    #[test]
    #[should_panic(expected = "data codeword >= 113")]
    fn encode_panics_on_out_of_range_codeword() {
        let data = [0u32, 113];
        let _ = encode(&data, 2);
    }

    /// Stage 11.A8c — pin `build_tables` antilog/log inverse-pair
    /// invariant. Kills the `* 3` / `% P` arithmetic and the
    /// `i as u8` cast mutations on lines 36-40.
    #[test]
    fn build_tables_antilog_log_are_inverses() {
        let (antilog, log) = build_tables();

        // antilog[0] = 1 (3^0 = 1).
        assert_eq!(antilog[0], 1);

        // log[1] = 0 (since 3^0 = 1, log of 1 is 0).
        assert_eq!(log[1], 0);

        // antilog[1] = 3 (3^1 = 3).
        assert_eq!(antilog[1], 3);
        assert_eq!(log[3], 1);

        // antilog[2] = 9 (3^2 = 9).
        assert_eq!(antilog[2], 9);
        assert_eq!(log[9], 2);

        // Inverse pair: for every non-zero v in [1, P-1], log[antilog[v]] = v
        // (when v < ORDER). Equivalently antilog[log[x]] = x for x in [1, P-1].
        for v in 1..=(P as u8 - 1) {
            let l = log[v as usize];
            let back = antilog[l as usize];
            assert_eq!(back, v, "antilog[log[{v}]] = {back}, want {v}");
        }
    }

    /// Stage 11.A8c — pin `generator_poly` shape and leading
    /// coefficient. Kills function-replacement mutants on line 48.
    #[test]
    fn generator_poly_shape() {
        // nck=0 → [1] (just the leading 1).
        let g0 = generator_poly(0);
        assert_eq!(g0, vec![1]);

        // nck=1 → [1, ?] (leading 1 + a single root).
        let g1 = generator_poly(1);
        assert_eq!(g1.len(), 2);
        assert_eq!(g1[0], 1, "leading coeff must be 1");

        // nck=4 → 5 coefficients with leading 1.
        let g4 = generator_poly(4);
        assert_eq!(g4.len(), 5);
        assert_eq!(g4[0], 1);

        // All coefficients are valid field elements (< P=113).
        for &c in &g4 {
            assert!(c < P, "coeff {c} >= P={}", P);
        }
    }
}
