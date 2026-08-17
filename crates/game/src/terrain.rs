//! Ground-layer generation. Placeholder scope: a flat field with
//! deterministic per-tile visual variation. Real zone/worldgen replaces
//! [`TerrainGrid::generate`] without changing the read API.

/// Number of visual ground variants; the renderer must provide a tile for
/// each variant in `0..GROUND_VARIANTS`.
pub const GROUND_VARIANTS: u8 = 3;

/// A dense, row-major grid of ground tiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerrainGrid {
    width: u32,
    height: u32,
    variants: Vec<u8>,
}

impl TerrainGrid {
    pub fn generate(seed: u64, width: u32, height: u32) -> Self {
        let mut variants = Vec::with_capacity((width as usize) * (height as usize));
        for y in 0..height {
            for x in 0..width {
                variants.push(variant_for(seed, x, y));
            }
        }
        Self {
            width,
            height,
            variants,
        }
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Visual variant in `0..GROUND_VARIANTS` of the tile at `(x, y)`.
    ///
    /// # Panics
    /// If `(x, y)` is outside the grid.
    #[must_use]
    pub fn variant(&self, x: u32, y: u32) -> u8 {
        assert!(
            x < self.width && y < self.height,
            "tile ({x}, {y}) outside {}x{} grid",
            self.width,
            self.height
        );
        self.variants[(y * self.width + x) as usize]
    }

    /// Iterates `(x, y, variant)` over every tile, row-major.
    pub fn iter(&self) -> impl Iterator<Item = (u32, u32, u8)> + '_ {
        self.variants.iter().enumerate().map(|(i, &variant)| {
            let i = u32::try_from(i).expect("grid larger than u32 tile indices");
            (i % self.width, i / self.width, variant)
        })
    }
}

/// Deterministic per-tile variant: mostly the plain tile with occasional
/// variation, so the floor reads as one material rather than noise.
fn variant_for(seed: u64, x: u32, y: u32) -> u8 {
    let h = splitmix64(seed ^ ((u64::from(x) << 32) | u64::from(y)));
    match h % 100 {
        0..=69 => 0,
        70..=84 => 1,
        _ => 2,
    }
}

/// SplitMix64 finalizer (Steele et al.) used as a stateless position hash:
/// worldgen stays deterministic per seed with no RNG state to thread around.
fn splitmix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
