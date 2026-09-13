//! Domain-separated coordinate hashing.
//!
//! See `DESIGN.md` sections 8, 8.1, 8.2, and 25.4.
//!
//! WGVB never uses a mutable PRNG stream as the basis of terrain generation,
//! because that makes results depend on traversal order. Deterministic values
//! come from hashing a semantic path instead: a seed, a compile-time domain
//! identifier, and the coordinates involved.
//!
//! Both the domain derivation and the mixer are written out here rather than
//! taken from a dependency. A dependency's semver-minor release can change its
//! output and silently invalidate every world with no version bump on our side,
//! and `std::collections::hash_map::DefaultHasher` is documented as unstable
//! across Rust releases — it would pass every test today and invalidate every
//! persisted world the day the toolchain is upgraded.
//!
//! Every mixer operation uses `wrapping_mul` / `wrapping_add` / `wrapping_xor`.
//! A mixer written with plain `*` and `+` compiles and then panics on the first
//! debug-build test; that is the most common defect when porting hashing code
//! from Go, where integer arithmetic wraps silently.

use crate::Seed;

/// FNV-1a over a domain name, evaluated at compile time.
///
/// Stable by construction: the algorithm is written here, so no dependency can
/// change it. Domain identifiers therefore need no hand-assigned integers that
/// can drift or collide. Adding a domain never disturbs an existing one;
/// *renaming* one changes the world and is an algorithm version change.
#[must_use]
pub const fn domain(name: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < name.len() {
        hash ^= name[i] as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    hash
}

/// Domain: broad continental land/ocean structure.
pub const DOM_CONTINENTALNESS: u64 = domain(b"continentalness");
/// Domain: regional uplift applied on top of continentalness.
pub const DOM_REGIONAL_ELEVATION: u64 = domain(b"regional-elevation");
/// Domain: local relief and roughness.
pub const DOM_RELIEF: u64 = domain(b"relief");
/// Domain: moisture field.
pub const DOM_MOISTURE: u64 = domain(b"moisture");
/// Domain: heat field.
pub const DOM_TEMPERATURE: u64 = domain(b"temperature");
/// Domain: fine terrain detail.
pub const DOM_TERRAIN_DETAIL: u64 = domain(b"terrain-detail");
/// Domain: per-region character selection.
pub const DOM_REGION_STYLE: u64 = domain(b"region-style");
/// Domain: ridge orientation, stored as a unit vector rather than an angle.
pub const DOM_RIDGE_ORIENTATION: u64 = domain(b"ridge-orientation");
/// Domain: basin influence.
pub const DOM_BASIN: u64 = domain(b"basin");
/// Domain: volcanic tendency.
pub const DOM_VOLCANIC: u64 = domain(b"volcanic");

/// Odd increment from SplitMix64, the fractional part of the golden ratio
/// scaled to 64 bits. Separates successive inputs so two coordinates cannot
/// cancel each other.
const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;

/// The SplitMix64 finalizer: an avalanche step where every input bit affects
/// every output bit.
///
/// Written out here for the reasons in the module comment. All arithmetic is
/// explicitly wrapping.
#[inline]
#[must_use]
const fn mix64(z: u64) -> u64 {
    let z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    let z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Hashes a seed, a domain, and `N` integers to a `u64`.
///
/// Rust has no variadics, which is an improvement here: fixed arity makes every
/// call site explicit about how many coordinates feed the hash. Inputs are
/// folded in a fixed order, so `hash_n(s, d, [a, b])` and `hash_n(s, d, [b, a])`
/// differ, and arities do not alias.
#[inline]
#[must_use]
pub const fn hash_n<const N: usize>(seed: Seed, domain: u64, values: [i64; N]) -> u64 {
    let mut acc = mix64(seed ^ domain);
    let mut i = 0;
    while i < N {
        // `cast_unsigned` is a reinterpretation, not a numeric conversion: a
        // negative coordinate keeps its bit pattern, which is what the mixer
        // wants. Negative coordinates are first-class here.
        acc = mix64(
            acc.wrapping_add(GAMMA)
                .wrapping_add(values[i].cast_unsigned()),
        );
        i += 1;
    }
    acc
}

/// Hashes a seed, a domain, and two integers. See [`hash_n`].
#[inline]
#[must_use]
pub const fn hash2(seed: Seed, domain: u64, a: i64, b: i64) -> u64 {
    hash_n(seed, domain, [a, b])
}

/// Hashes a seed, a domain, and three integers. See [`hash_n`].
#[inline]
#[must_use]
pub const fn hash3(seed: Seed, domain: u64, a: i64, b: i64, c: i64) -> u64 {
    hash_n(seed, domain, [a, b, c])
}

/// `2^-53`, the exact scale for a 53-bit significand.
const SCALE_53: f64 = 1.0 / 9_007_199_254_740_992.0;

/// Converts a hash to an `f64` in `[0, 1)`.
///
/// Takes the top 53 bits — the width of the `f64` significand — and scales by an
/// exact power of two, so every result is exactly representable and the
/// conversion is bit-identical on every target. Never
/// `h as f64 / u64::MAX as f64`, which is a rounded division by a value `f64`
/// cannot represent.
#[inline]
#[must_use]
pub const fn unit_f64(h: u64) -> f64 {
    let top53 = h >> 11;
    #[expect(
        clippy::cast_precision_loss,
        reason = "top53 < 2^53, so the conversion to f64 is exact"
    )]
    let significand = top53 as f64;
    significand * SCALE_53
}

/// Converts a hash to an `f64` in `[-1, 1)`.
///
/// Uses only multiplication and subtraction on an exactly representable value.
#[inline]
#[must_use]
pub const fn signed_unit_f64(h: u64) -> f64 {
    unit_f64(h) * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// A deterministic sample of coordinates, including negatives and extremes.
    fn sample_coords() -> Vec<(i64, i64)> {
        let mut out = Vec::new();
        for q in -8..=8_i64 {
            for r in -8..=8_i64 {
                out.push((q, r));
            }
        }
        for v in [
            i64::MIN,
            i64::MIN + 1,
            -1_000_000,
            0,
            1_000_000,
            i64::MAX - 1,
            i64::MAX,
        ] {
            out.push((v, 0));
            out.push((0, v));
            out.push((v, v));
        }
        // Deduplicated and sorted: the extremes overlap the grid at the origin,
        // and a collision test must not count a repeated input as a collision.
        out.sort_unstable();
        out.dedup();
        out
    }

    #[test]
    fn domain_matches_fnv_1a_computed_independently() {
        // FNV-1a 64-bit offset basis 0xcbf29ce484222325, prime 0x100000001b3,
        // computed outside this crate.
        assert_eq!(domain(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(DOM_CONTINENTALNESS, 0xe6a3_e327_f102_50f5);
        assert_eq!(DOM_REGIONAL_ELEVATION, 0xe3b0_ad5b_6320_39e6);
        assert_eq!(DOM_RELIEF, 0xfa18_9c4d_af36_7720);
        assert_eq!(DOM_MOISTURE, 0x4532_a032_9655_240f);
        assert_eq!(DOM_TEMPERATURE, 0x5565_75c1_ce10_7955);
        assert_eq!(DOM_TERRAIN_DETAIL, 0xa90f_24aa_e7b9_a828);
        assert_eq!(DOM_REGION_STYLE, 0x4115_6f2b_a1d9_a7d5);
        assert_eq!(DOM_RIDGE_ORIENTATION, 0x0f5b_ac88_66b1_037b);
        assert_eq!(DOM_BASIN, 0xd6e8_5182_6dfb_0aa6);
        assert_eq!(DOM_VOLCANIC, 0x3208_6c8b_90c6_35b8);
    }

    #[test]
    fn every_domain_constant_is_distinct() {
        let all = all_domains();
        let unique: HashSet<u64> = all.iter().copied().collect();
        assert_eq!(unique.len(), all.len());
    }

    fn all_domains() -> Vec<u64> {
        vec![
            DOM_CONTINENTALNESS,
            DOM_REGIONAL_ELEVATION,
            DOM_RELIEF,
            DOM_MOISTURE,
            DOM_TEMPERATURE,
            DOM_TERRAIN_DETAIL,
            DOM_REGION_STYLE,
            DOM_RIDGE_ORIENTATION,
            DOM_BASIN,
            DOM_VOLCANIC,
        ]
    }

    #[test]
    fn hashing_is_deterministic() {
        for (q, r) in sample_coords() {
            let first = hash2(0x1234_5678_9abc_def0, DOM_RELIEF, q, r);
            let second = hash2(0x1234_5678_9abc_def0, DOM_RELIEF, q, r);
            assert_eq!(first, second, "({q}, {r})");
        }
    }

    #[test]
    fn hashing_is_a_const_expression() {
        // Domains and hashes must be usable in const context, which also proves
        // the mixer contains no floating point and no runtime dispatch.
        const H: u64 = hash2(7, DOM_BASIN, -3, 9);
        assert_eq!(H, hash2(7, DOM_BASIN, -3, 9));
    }

    #[test]
    fn extreme_inputs_do_not_overflow_in_a_debug_build() {
        // The whole point of the wrapping-operation rule. A non-wrapping mixer
        // panics here rather than returning a wrong answer.
        for (q, r) in sample_coords() {
            for seed in [0, 1, u64::MAX, u64::MAX / 3] {
                for d in all_domains() {
                    let _ = hash3(seed, d, q, r, q.wrapping_mul(r));
                }
            }
        }
    }

    #[test]
    fn domains_separate_identical_inputs() {
        // Two systems sampling the same coordinate must not share a value, so a
        // change to one field's inputs cannot perturb another's output.
        let mut seen: HashSet<u64> = HashSet::new();
        let mut collisions = 0;
        for (q, r) in sample_coords() {
            for d in all_domains() {
                if !seen.insert(hash2(0xdead_beef, d, q, r)) {
                    collisions += 1;
                }
            }
        }
        assert_eq!(collisions, 0, "domain-separated hashes collided");
    }

    #[test]
    fn seeds_separate_identical_inputs() {
        let mut seen: HashSet<u64> = HashSet::new();
        for seed in 0..512_u64 {
            assert!(seen.insert(hash2(seed, DOM_MOISTURE, 3, -4)), "seed {seed}");
        }
    }

    #[test]
    fn argument_order_and_arity_matter() {
        let s = 99;
        assert_ne!(
            hash2(s, DOM_TEMPERATURE, 1, 2),
            hash2(s, DOM_TEMPERATURE, 2, 1)
        );
        assert_ne!(
            hash_n(s, DOM_TEMPERATURE, [1_i64]),
            hash_n(s, DOM_TEMPERATURE, [1_i64, 0])
        );
        assert_ne!(
            hash_n::<0>(s, DOM_TEMPERATURE, []),
            hash_n(s, DOM_TEMPERATURE, [0_i64])
        );
    }

    #[test]
    fn neighboring_coordinates_do_not_produce_neighboring_hashes() {
        // A weak mixer shows up as a visible lattice in generated terrain. Adjacent
        // coordinates must differ in roughly half their output bits.
        let mut total_bits = 0_u32;
        let mut samples = 0_u32;
        for q in -32..32_i64 {
            for r in -32..32_i64 {
                let a = hash2(42, DOM_CONTINENTALNESS, q, r);
                let b = hash2(42, DOM_CONTINENTALNESS, q + 1, r);
                total_bits += (a ^ b).count_ones();
                samples += 1;
            }
        }
        let mean = f64::from(total_bits) / f64::from(samples);
        assert!((26.0..38.0).contains(&mean), "mean flipped bits {mean}");
    }

    #[test]
    fn every_output_bit_avalanches() {
        // Flip one input bit and count how often each output bit changes. A
        // well-behaved finalizer lands near half for every position.
        let mut flips = [0_u32; 64];
        let trials = 1_024_u32;
        for i in 0..trials {
            let value = i64::from(i).wrapping_mul(0x0123_4567).wrapping_sub(1 << 40);
            for bit in 0..64 {
                let a = hash2(0xa5a5_a5a5, DOM_TERRAIN_DETAIL, value, 7);
                let b = hash2(0xa5a5_a5a5, DOM_TERRAIN_DETAIL, value ^ (1_i64 << bit), 7);
                let diff = a ^ b;
                for (position, count) in flips.iter_mut().enumerate() {
                    *count += u32::from((diff >> position) & 1 == 1);
                }
            }
        }
        let expected = f64::from(trials) * 64.0;
        for (position, count) in flips.iter().enumerate() {
            let rate = f64::from(*count) / expected;
            assert!(
                (0.4..0.6).contains(&rate),
                "output bit {position} flip rate {rate}"
            );
        }
    }

    #[test]
    fn unit_values_stay_in_range_and_use_the_top_53_bits() {
        assert_eq!(unit_f64(0), 0.0);
        // The top bit alone is exactly one half.
        assert_eq!(unit_f64(1 << 63), 0.5);
        assert_eq!(unit_f64(1 << 62), 0.25);
        // The largest possible value is 1 - 2^-53, strictly below one. A
        // division by `u64::MAX` would round to exactly 1.0 here.
        assert_eq!(unit_f64(u64::MAX), 1.0 - SCALE_53);
        assert!(unit_f64(u64::MAX) < 1.0);
        // The low 11 bits cannot matter.
        assert_eq!(unit_f64(0x0000_0000_0000_07ff), 0.0);

        for (q, r) in sample_coords() {
            let u = unit_f64(hash2(5, DOM_VOLCANIC, q, r));
            assert!((0.0..1.0).contains(&u), "({q}, {r}) gave {u}");
            let s = signed_unit_f64(hash2(5, DOM_VOLCANIC, q, r));
            assert!((-1.0..1.0).contains(&s), "({q}, {r}) gave {s}");
            assert_eq!(s, u * 2.0 - 1.0);
        }
    }

    #[test]
    fn unit_values_are_roughly_uniform() {
        // Distribution check, not a knife-edge threshold test: a mean far from
        // one half or an empty decile would mean the conversion is wrong.
        let mut buckets = [0_u32; 10];
        let mut sum = 0.0_f64;
        let count = 20_000_u32;
        for i in 0..count {
            let value = i64::from(i) - i64::from(count) / 2;
            let u = unit_f64(hash2(0xc0ffee, DOM_REGION_STYLE, value, value * 3));
            sum += u;
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "u is in [0, 1), so u * 10.0 is non-negative and below ten"
            )]
            let index = (u * 10.0) as usize;
            buckets[index] += 1;
        }
        let mean = sum / f64::from(count);
        assert!((0.48..0.52).contains(&mean), "mean {mean}");
        let low = f64::from(count) * 0.08;
        for (index, bucket) in buckets.iter().enumerate() {
            assert!(
                f64::from(*bucket) > low,
                "decile {index} held only {bucket}"
            );
        }
    }
}
