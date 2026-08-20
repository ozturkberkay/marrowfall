//! World generation: the shape of the world, with no engine, no I/O and no
//! threads.
//!
//! Three rules hold everywhere in this crate, and every guarantee the streaming
//! world depends on comes from them:
//!
//! 1. **Position pure.** Every value is a function of the world seed and a
//!    coordinate. Nothing reads a neighbouring chunk, so generating one chunk
//!    can never trigger generating another, and chunks can be produced in any
//!    order on any thread.
//! 2. **No floats in the result.** Intermediate maths may use them; what gets
//!    stored is integral. A world whose stored form holds no float cannot drift
//!    by a rounding difference.
//! 3. **No non-deterministic calls.** No wall clock, no OS randomness, and none
//!    of the standard library's transcendental functions, whose precision Rust
//!    documents as varying by platform, by compiler version, and even between
//!    two calls in one run.
//!
//! It is a crate rather than a module of `game` because three consumers need it:
//! the simulation for collision, the frontend for painting, and the offline
//! preview tool, which must not drag in an ECS to draw a picture.

mod chunk;
mod hash;
mod height;
mod region;
mod rules;
mod site;
mod tile;
mod world;

pub use chunk::{
    CHUNK_TILES, ChunkCoord, ChunkView, GHOST_WIDTH, GRID_AREA, GRID_SIDE, generate_chunk, tile_at,
};
pub use hash::{Domain, derive, derive_with};
pub use height::height_at;
pub use region::{Region, RegionPoint, region_at};
pub use rules::{
    BiomeId, BiomeRow, Error, HEIGHT_RANGE, MaterialRow, SiteClassId, SiteClassRow, SiteId,
    SiteRow, Tables, TierRow, WorldRules, parse,
};
pub use site::{Site, site_at, sites_near};
pub use tile::{MaterialId, Tile, TileFlags};
pub use world::World;

/// Re-exported because they appear in this crate's public API: a caller must be
/// able to name a tile coordinate without picking its own `glam` version.
pub use glam::{I64Vec2, IVec2};
