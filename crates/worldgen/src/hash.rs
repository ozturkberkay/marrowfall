//! The one source of randomness in world generation: a stateless hash of a
//! position, not a stream.
//!
//! A stream would force every generator to be run in a fixed order, because
//! each draw depends on the last. A hash lets any tile be asked for at any
//! time, which is what makes chunks generatable in any order and on any
//! thread. It also means there is no RNG state to thread through a call chain,
//! and no library whose value stability we depend on.

/// What a derivation is for.
///
/// Two systems hashing the same coordinate under the same tag get the same
/// number, so every independent decision needs its own tag. Elevation and
/// moisture sharing one would make them perfectly correlated.
///
/// The numbers are permanent. Renumbering a variant changes every world ever
/// generated from every seed.
#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    Height = 1,
    RegionJitter = 2,
    RegionBiome = 3,
    Stray = 4,
    /// Reserved for the site lattice, which is the next design. Reserved rather
    /// than added later so its worlds do not shift when it arrives.
    Site = 5,
    RegionWarp = 6,
    SiteKind = 7,
}

/// Odd multipliers, so each is invertible modulo 2^64 and loses no input bits.
/// Distinct per axis, which is what stops the world mirroring about `x == y`.
const SPREAD_DOMAIN: u64 = 0xD6E8_FEB8_6659_FD93;
const SPREAD_X: u64 = 0x9E37_79B9_7F4A_7C15;
const SPREAD_Y: u64 = 0xC2B2_AE3D_27D4_EB4F;
const SPREAD_VARIANT: u64 = 0xFF51_AFD7_ED55_8CCD;

/// A number derived from a seed, a purpose and a tile coordinate.
///
/// Pure: the same arguments always give the same answer, on every platform and
/// in any order. Uses only wrapping integer arithmetic, so there is no float to
/// round differently and no library call to change under us.
#[must_use]
pub fn derive(seed: u64, domain: Domain, x: i32, y: i32) -> u64 {
    derive_with(seed, domain, 0, x, y)
}

/// A number derived from a seed, a purpose, a *variant* of that purpose, and a
/// tile coordinate.
///
/// `variant` is for a domain with several independent users, such as one site
/// class per kind of landmark. It is mixed in as its own input for the same
/// reason `domain` is: xor-ing it into a coordinate instead would make class A
/// at one cell bit-identical to class B at a shifted cell, so two classes would
/// place their sites in lockstep along every row.
#[must_use]
pub fn derive_with(seed: u64, domain: Domain, variant: u64, x: i32, y: i32) -> u64 {
    // Multiplied rather than xor-ed in, so no pair of (seed, domain) can
    // collide with a different pair.
    let purpose =
        seed ^ (domain as u64).wrapping_mul(SPREAD_DOMAIN) ^ variant.wrapping_mul(SPREAD_VARIANT);
    // `as u32` sign-extends then truncates, which is what makes a negative
    // coordinate well defined rather than platform dependent.
    let x = u64::from(x as u32).wrapping_mul(SPREAD_X);
    let y = u64::from(y as u32).wrapping_mul(SPREAD_Y);
    // Twice, because one pass of a finalizer is not enough to mix a sparse 2D
    // key: the constant was chosen to mix an already-incrementing counter.
    splitmix64(splitmix64(purpose ^ x) ^ y)
}

/// SplitMix64 finalizer (Steele et al.).
fn splitmix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
