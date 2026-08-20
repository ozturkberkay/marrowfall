//! Points of interest, and the rules that decide where they go.
//!
//! A coarse lattice over the infinite world: one cell of a class holds at most
//! one site, and whether it does is a pure function of the cell's coordinates.
//! That is what lets a chunk find every site that reaches it without asking a
//! neighbour, and what makes the placement reproducible from the seed alone.
//!
//! The rules are the point. A class declares its `spacing`, which is the lattice
//! pitch and so the average gap, and its `separation`, which is the gap the
//! placement guarantees. Both terms are Minecraft's, from its structure sets,
//! because the concept is the same one.
//!
//! What a site does to the ground is deliberately not here. Until there are
//! authored footprints to stamp, a site is a position and a kind, which is
//! everything the spacing rules need and nothing they do not.

use glam::IVec2;

use crate::hash::{Domain, derive_with};
use crate::region::region_at;
use crate::rules::{SiteClassId, SiteId};
use crate::world::World;

/// One placed point of interest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Site {
    pub kind: SiteId,
    pub class: SiteClassId,
    /// The centre tile, in world coordinates.
    pub at: IVec2,
}

/// The site in one lattice cell of one class, if that cell holds one.
///
/// Pure: reads no chunk and no neighbouring cell, so it can be asked in any
/// order and on any thread.
#[must_use]
pub fn site_at(world: &World, class: SiteClassId, cell: IVec2) -> Option<Site> {
    let rules = world.rules();
    let row = rules.site_class(class);

    // The class is a variant of the domain rather than something folded into a
    // coordinate. Xor-ing it into `cell` instead would make two classes place
    // their sites in lockstep along every row.
    let h = derive_with(
        world.seed(),
        Domain::Site,
        u64::from(class.0),
        cell.x,
        cell.y,
    );

    if (h % 100) as u8 >= row.fill_pct {
        return None;
    }

    // The site sits anywhere in its cell except a margin of half the separation
    // at each edge. Two sites in neighbouring cells are then at least
    // `separation + 1` apart, which is the guarantee rather than an average.
    let margin = row.separation / 2;
    let free = row.spacing - row.separation;
    let offset = IVec2::new(
        ((h >> 8) % free.unsigned_abs() as u64) as i32,
        ((h >> 32) % free.unsigned_abs() as u64) as i32,
    );
    let at = cell * row.spacing + offset + IVec2::splat(margin);

    // Gates, applied after the position is known because both depend on it.
    if distance_from_origin(at) < row.min_from_spawn {
        return None;
    }
    let tier = region_at(world, at).tier;
    if tier < row.tier_lo || tier > row.tier_hi {
        return None;
    }

    Some(Site {
        kind: pick_kind(world, class, cell, h),
        class,
        at,
    })
}

/// Every site of every class whose centre lies within `radius` tiles of `tile`.
///
/// The window is a fixed number of cells per class, computed from the pitch, so
/// this is a bounded walk and never a search.
#[must_use]
pub fn sites_near(world: &World, tile: IVec2, radius: i32) -> Vec<Site> {
    let rules = world.rules();
    let mut out = Vec::new();
    for class in rules.site_classes() {
        let row = rules.site_class(class);
        // One cell of slack, because a cell's site can sit anywhere inside it.
        let reach = radius.div_euclid(row.spacing) + 1;
        let home = IVec2::new(
            tile.x.div_euclid(row.spacing),
            tile.y.div_euclid(row.spacing),
        );
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                let Some(site) = site_at(world, class, home + IVec2::new(dx, dy)) else {
                    continue;
                };
                let offset = site.at - tile;
                if offset.x.abs() <= radius && offset.y.abs() <= radius {
                    out.push(site);
                }
            }
        }
    }
    out
}

/// Distance from the world origin, in tiles.
fn distance_from_origin(at: IVec2) -> i64 {
    let (x, y) = (i64::from(at.x), i64::from(at.y));
    ((x * x + y * y) as u128).isqrt() as i64
}

/// Which kind of site this cell holds, by weight among its class's kinds.
fn pick_kind(world: &World, class: SiteClassId, cell: IVec2, h: u64) -> SiteId {
    let rules = world.rules();
    let kinds = rules.sites_in(class);
    let total: u64 = kinds
        .iter()
        .map(|&id| u64::from(rules.site(id).weight))
        .sum();
    // A second draw, so the kind does not correlate with whether a cell is
    // filled at all.
    let mut roll = derive_with(
        world.seed(),
        Domain::SiteKind,
        u64::from(class.0),
        cell.x,
        cell.y,
    )
    .wrapping_add(h)
        % total;
    // All but the last, then fall through, so no branch is unreachable. The
    // tables guarantee a class has at least one kind.
    let (&last, rest) = kinds.split_last().expect("a class with no kinds");
    for &id in rest {
        let weight = u64::from(rules.site(id).weight);
        if roll < weight {
            return id;
        }
        roll -= weight;
    }
    last
}
