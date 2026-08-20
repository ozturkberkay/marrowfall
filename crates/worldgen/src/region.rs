//! Regions: the patchwork that gives the world its biomes and its difficulty
//! gradient.
//!
//! A jittered lattice of points. Every tile belongs to its nearest point, so the
//! world partitions into irregular patches, each holding exactly one biome. The
//! patch takes its tier from its own point's distance to the origin, which is
//! what makes danger rise outward while leaving the shapes irregular.
//!
//! Position pure throughout: a region is a function of the tile's coordinate, so
//! no chunk ever needs a neighbour.

use glam::{I64Vec2, IVec2};

use crate::hash::{Domain, derive};
use crate::rules::{BiomeId, TierRow};
use crate::world::World;

/// One lattice point, and the cell that owns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionPoint {
    /// The cell is the region's identity, and what every roll about the region
    /// hashes from. Small, so it never overflows a coordinate.
    pub cell: IVec2,
    /// Where the point sits in world tiles. `i64`, because a cell index times
    /// the pitch outruns `i32` before the world runs out.
    pub at: I64Vec2,
}

impl RegionPoint {
    /// Distance from the world origin, in tiles.
    #[must_use]
    pub fn distance_tiles(&self) -> i64 {
        // i128 for the squares: at the far edge of an i32 tile grid the sum of
        // two i64 squares would overflow.
        let (x, y) = (i128::from(self.at.x), i128::from(self.at.y));
        let d2 = (x * x + y * y) as u128;
        // Integer square root, so distance never depends on float rounding.
        d2.isqrt() as i64
    }
}

/// Which biome a patch of world holds, and how dangerous it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub point: RegionPoint,
    pub tier: u8,
    pub biome: BiomeId,
}

/// Which way a region strayed from its distance tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stray {
    None,
    Harder,
    Easier,
}

/// The region containing `tile`.
#[must_use]
pub fn region_at(world: &World, tile: IVec2) -> Region {
    let point = nearest_point(world, tile);
    let tier = tier_of(world, point);
    Region {
        point,
        tier,
        biome: pick_biome(world, point, tier),
    }
}

/// The lattice point nearest `tile`.
///
/// Searches the surrounding 3 by 3 cells, which is provably enough because
/// `parse` bounds jitter to half the pitch: the owning cell's point is at most
/// 1.06 pitches away, while any point two cells out is at least 1.25.
fn nearest_point(world: &World, tile: IVec2) -> RegionPoint {
    let pitch = world.rules().region_pitch();
    // Bend the query before looking it up. Straight Voronoi edges read as
    // artificial, and warping the input is how a boundary becomes an organic
    // line without giving up the one-point-per-cell structure the partition
    // depends on. The warp is a small fraction of the pitch, so the nearest
    // point is still inside the 3 by 3 block searched below.
    let here = warped(world, tile);
    let home = IVec2::new(
        (here.x.div_euclid(i64::from(pitch))) as i32,
        (here.y.div_euclid(i64::from(pitch))) as i32,
    );
    let mut best: Option<(u128, RegionPoint)> = None;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let point = point_of(world, home + IVec2::new(dx, dy));
            let offset = point.at - here;
            let d2 = (i128::from(offset.x).pow(2) + i128::from(offset.y).pow(2)) as u128;
            // Strictly nearer, so a tie resolves to the earlier cell in a fixed
            // scan order rather than to whichever was visited last.
            if best.is_none_or(|(seen, _)| d2 < seen) {
                best = Some((d2, point));
            }
        }
    }
    // The loop always runs, so this is the nine candidates' winner.
    best.map_or_else(|| point_of(world, home), |(_, point)| point)
}

/// The tile coordinate the lattice is actually asked about.
///
/// Rounds to whole tiles, so the answer is an integer function of an integer
/// input and no float ever reaches a stored value.
fn warped(world: &World, tile: IVec2) -> I64Vec2 {
    let (x, y) = world
        .warp_noise()
        .domain_warp_2d(tile.x as f32, tile.y as f32);
    I64Vec2::new(x.round() as i64, y.round() as i64)
}

/// Where one cell's point sits: its centre, displaced by a hashed offset.
fn point_of(world: &World, cell: IVec2) -> RegionPoint {
    let rules = world.rules();
    let pitch = i64::from(rules.region_pitch());
    let half = pitch / 2;
    // Jitter is a percentage of half the pitch, so 100 puts a point anywhere in
    // its own cell and no further. `parse` caps it there.
    let span = half * i64::from(rules.region_jitter_pct()) / 100;
    let centre = I64Vec2::new(
        i64::from(cell.x) * pitch + half,
        i64::from(cell.y) * pitch + half,
    );
    let at = if span == 0 {
        centre
    } else {
        let h = derive(world.seed(), Domain::RegionJitter, cell.x, cell.y);
        let width = (2 * span + 1) as u64;
        centre + I64Vec2::new((h % width) as i64 - span, ((h >> 32) % width) as i64 - span)
    };
    RegionPoint { cell, at }
}

/// The region's difficulty tier.
fn tier_of(world: &World, point: RegionPoint) -> u8 {
    let rules = world.rules();
    // The home bubble is enforced on the region, with one region radius of
    // slack. A per tile radius cannot be guaranteed by a per region rule: a tile
    // just inside the bubble can belong to a region whose point sits outside it,
    // and would inherit that point's tier. Taking the minimum of the two
    // distances instead would split one region's tier at the bubble's edge and
    // break the one-patch-one-tier property everything else relies on.
    let slack = i64::from(rules.region_pitch()) / 2;
    if point.distance_tiles() <= rules.home_bubble() + slack {
        return 0;
    }
    let base = rules.tier_for(point.distance_tiles());
    let band = rules.band_of(base);
    match stray_of(world, point, band) {
        Stray::None => base,
        Stray::Harder => base.saturating_add(band.harder_stray).min(rules.max_tier()),
        Stray::Easier => base.saturating_sub(band.easier_stray),
    }
}

/// Whether this region strays from its distance tier, and which way.
///
/// The band's percentage decides whether it strays at all. When both directions
/// are open the choice is a fair coin, which is the only split the table does
/// not spell out.
fn stray_of(world: &World, point: RegionPoint, band: &TierRow) -> Stray {
    let h = derive(world.seed(), Domain::Stray, point.cell.x, point.cell.y);
    if (h % 100) as u8 >= band.stray_pct {
        return Stray::None;
    }
    match (band.harder_stray > 0, band.easier_stray > 0) {
        (false, false) => Stray::None,
        (true, false) => Stray::Harder,
        (false, true) => Stray::Easier,
        (true, true) if (h >> 8) & 1 == 0 => Stray::Harder,
        (true, true) => Stray::Easier,
    }
}

/// One of the tier's biomes, by weight.
fn pick_biome(world: &World, point: RegionPoint, tier: u8) -> BiomeId {
    let rules = world.rules();
    let ids = rules.biomes_in(tier);
    let total: u64 = ids
        .iter()
        .map(|&id| u64::from(rules.biome(id).weight))
        .sum();
    // `parse` rejects a zero weight and an empty tier, so the total is positive.
    let mut roll = derive(
        world.seed(),
        Domain::RegionBiome,
        point.cell.x,
        point.cell.y,
    ) % total;
    // All but the last, then fall through to it. Walking every entry and keeping
    // a fallback would leave a branch that cannot run, because the weights sum
    // to exactly `total` and the roll is below it.
    let (&last, rest) = ids
        .split_last()
        .expect("parse rejects a tier with no biomes");
    for &id in rest {
        let weight = u64::from(rules.biome(id).weight);
        if roll < weight {
            return id;
        }
        roll -= weight;
    }
    last
}
