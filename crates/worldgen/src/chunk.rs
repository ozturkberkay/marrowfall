//! Chunks: the unit the world is generated, streamed and painted in.
//!
//! A chunk carries one tile more than it owns on every side: a ring of ghost
//! cells, the standard name for copied edge data in a decomposed grid. The
//! frontend needs a tile's neighbours to pick its sprite (a cliff face exists
//! only relative to a lower tile), and at a chunk's edge those neighbours belong
//! to a chunk that may not be resident. Generating the ring here is free,
//! because [`crate::tile_at`] is a pure function of position, and it means the
//! frontend never waits on a neighbour.
//!
//! The consequence worth naming: one chunk's ghost cells and its neighbour's
//! interior
//! describe the same tiles and always agree, because both come from the same
//! function rather than from each other.

use glam::IVec2;

use crate::region::region_at;
use crate::tile::Tile;
use crate::world::World;

/// Tiles along one edge of a chunk.
///
/// Sized against the screen: at the project's 2560 by 1440 viewport a screen
/// shows about 400 tiles, so a chunk is roughly two and a half screens. Small
/// enough that generating one is never a visible hitch, large enough that few
/// are resident.
pub const CHUNK_TILES: i32 = 32;

/// How far the ghost cells reach. One ring is enough for every neighbour
/// comparison,
/// including diagonals, because the ring's corners are part of it.
pub const GHOST_WIDTH: i32 = 1;

/// Tiles along one edge of the grid including its ghost cells.
pub const GRID_SIDE: usize = (CHUNK_TILES + 2 * GHOST_WIDTH) as usize;

/// Tiles in the grid including its ghost cells.
pub const GRID_AREA: usize = GRID_SIDE * GRID_SIDE;

/// Which chunk, in chunk units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkCoord {
    pub x: i32,
    pub y: i32,
}

impl ChunkCoord {
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// The chunk containing `tile`.
    ///
    /// Floor division, not truncating division, so tile -1 belongs to chunk -1
    /// rather than to chunk 0. Truncation would make two chunks share a row.
    #[must_use]
    pub fn of(tile: IVec2) -> Self {
        Self {
            x: tile.x.div_euclid(CHUNK_TILES),
            y: tile.y.div_euclid(CHUNK_TILES),
        }
    }

    /// The world tile at this chunk's local origin.
    #[must_use]
    pub fn origin(self) -> IVec2 {
        IVec2::new(
            self.x.wrapping_mul(CHUNK_TILES),
            self.y.wrapping_mul(CHUNK_TILES),
        )
    }
}

/// One chunk's tiles, plus one ring of ghost cells on every side.
///
/// Local coordinates run `-1..=32`: `0..32` is the chunk, and the ring outside
/// outside it holds the ghost cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkView {
    pub coord: ChunkCoord,
    /// Row major over the grid including its ghost cells. Private, so the index
    /// arithmetic that
    /// turns a signed local coordinate into an offset lives in one place.
    tiles: Vec<Tile>,
}

impl ChunkView {
    /// The tile at a local coordinate, or `None` outside the stored grid.
    #[must_use]
    pub fn tile(&self, local: IVec2) -> Option<Tile> {
        let (x, y) = (local.x + GHOST_WIDTH, local.y + GHOST_WIDTH);
        let side = GRID_SIDE as i32;
        if !(0..side).contains(&x) || !(0..side).contains(&y) {
            return None;
        }
        self.tiles.get((y * side + x) as usize).copied()
    }

    /// Every tile the chunk owns, with its local coordinate, row major. The
    /// ghost cells are deliberately excluded: they exist to be read, not painted.
    pub fn interior(&self) -> impl Iterator<Item = (IVec2, Tile)> + '_ {
        (0..CHUNK_TILES).flat_map(move |y| {
            (0..CHUNK_TILES).filter_map(move |x| {
                let local = IVec2::new(x, y);
                self.tile(local).map(|tile| (local, tile))
            })
        })
    }

    /// Builds a view from tiles that did not come from `generate_chunk`.
    ///
    /// For tests, which need a chunk containing one specific obstacle rather than
    /// whatever the generator produced.
    ///
    /// # Panics
    /// If `tiles` is not exactly one full grid, ghost cells included.
    #[must_use]
    pub fn from_tiles(coord: ChunkCoord, tiles: Vec<Tile>) -> Self {
        assert_eq!(tiles.len(), GRID_AREA, "a chunk view is one full grid");
        Self { coord, tiles }
    }

    /// The raw slab, for hashing and for building a frontend's paint buffer.
    #[must_use]
    pub fn tiles(&self) -> &[Tile] {
        &self.tiles
    }
}

/// One tile, as a pure function of the world and its coordinate.
///
/// Reads no chunk, so it can be called for a neighbour's coordinate without
/// generating the neighbour. That is what makes cascading generation impossible.
#[must_use]
pub fn tile_at(world: &World, tile: IVec2) -> Tile {
    let region = region_at(world, tile);
    let biome = world.rules().biome(region.biome);
    Tile {
        material: biome.ground,
        height: crate::height_at(world, tile),
        flags: world.rules().material(biome.ground).flags,
    }
}

/// A chunk and its ghost cells.
///
/// Every tile comes from [`tile_at`], the ghost cells included, so the result
/// depends
/// on nothing but the world and the coordinate. Generating chunks in any order,
/// or more than once, gives the same answer.
#[must_use]
pub fn generate_chunk(world: &World, coord: ChunkCoord) -> ChunkView {
    let origin = coord.origin();
    let side = GRID_SIDE as i32;
    let mut tiles = Vec::with_capacity(GRID_AREA);
    for y in 0..side {
        for x in 0..side {
            let world_tile = origin + IVec2::new(x - GHOST_WIDTH, y - GHOST_WIDTH);
            tiles.push(tile_at(world, world_tile));
        }
    }
    ChunkView { coord, tiles }
}
