//! The seeded world: tuning data, the seed, and the noise built from it, held in
//! one value.
//!
//! One value and not three arguments, because a loose `seed` parameter could
//! silently disagree with the noise instance built from a different one. Noise
//! carries its seed internally, so the only way to keep them honest is to
//! construct them together.

use std::fmt;

use fastnoise_lite::{DomainWarpType, FastNoiseLite, FractalType, NoiseType};

use crate::hash::{Domain, derive};
use crate::rules::WorldRules;

/// How many octaves the height field layers. Three gives a readable silhouette
/// without turning plateaus into gravel.
const HEIGHT_OCTAVES: i32 = 3;

/// Tiles across one bend of a region boundary. Shorter makes the edges fussy,
/// longer makes them lazy curves.
const WARP_PERIOD_TILES: f32 = 400.0;

/// How far a boundary may bend, as a percentage of the region pitch. Well under
/// half, so the nearest region point stays inside the searched 3 by 3 block.
const WARP_PCT_OF_PITCH: i32 = 15;

/// Everything generation needs, and nothing it does not.
///
/// `Send + Sync` by construction: no interior mutability, so one `&World` can be
/// shared by the simulation, the frontend and any future worker.
///
/// `Debug` and `Clone` are written out rather than derived, because
/// `FastNoiseLite` implements neither. Cloning rebuilds the noise from the seed,
/// which is exactly what makes it equivalent.
pub struct World {
    rules: WorldRules,
    seed: u64,
    height_noise: FastNoiseLite,
    /// Bends region boundaries, so a biome edge is an organic line rather than
    /// the straight side of a Voronoi cell.
    warp_noise: FastNoiseLite,
}

impl World {
    #[must_use]
    pub fn new(rules: WorldRules, seed: u64) -> Self {
        let mut height_noise = FastNoiseLite::with_seed(noise_seed(seed, Domain::Height));
        height_noise.set_noise_type(Some(NoiseType::OpenSimplex2));
        height_noise.set_fractal_type(Some(FractalType::FBm));
        height_noise.set_fractal_octaves(Some(HEIGHT_OCTAVES));
        // Frequency lives in the coordinate, not here: each biome divides by its
        // own period, so one shared noise instance serves all of them.
        height_noise.set_frequency(Some(1.0));

        let mut warp_noise = FastNoiseLite::with_seed(noise_seed(seed, Domain::RegionWarp));
        warp_noise.set_domain_warp_type(Some(DomainWarpType::OpenSimplex2));
        warp_noise.set_frequency(Some(1.0 / WARP_PERIOD_TILES));
        // Amplitude in tiles, scaled off the pitch so the shape of a boundary
        // stays the same when the world's scale is retuned.
        let amp = (rules.region_pitch() * WARP_PCT_OF_PITCH / 100) as f32;
        warp_noise.set_domain_warp_amp(Some(amp));

        Self {
            rules,
            seed,
            height_noise,
            warp_noise,
        }
    }

    #[must_use]
    pub fn rules(&self) -> &WorldRules {
        &self.rules
    }

    #[must_use]
    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub(crate) fn height_noise(&self) -> &FastNoiseLite {
        &self.height_noise
    }

    pub(crate) fn warp_noise(&self) -> &FastNoiseLite {
        &self.warp_noise
    }
}

impl fmt::Debug for World {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The noise is a function of the seed, so printing the seed says
        // everything printing the tables of gradients would.
        f.debug_struct("World")
            .field("seed", &self.seed)
            .field("rules", &self.rules)
            .finish()
    }
}

impl Clone for World {
    fn clone(&self) -> Self {
        Self::new(self.rules.clone(), self.seed)
    }
}

/// Folds the world seed down to the `i32` the noise library takes.
///
/// Under a domain tag, because two noise instances that folded to the same
/// `i32` would produce identical fields: the domain collision defect one layer
/// down from tile hashing.
fn noise_seed(seed: u64, domain: Domain) -> i32 {
    derive(seed, domain, 0, 0) as i32
}
