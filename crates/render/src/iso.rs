//! The isometric mapping between tile space and screen space, in one place.
//!
//! Measured against Godot's isometric `TileMapLayer`, not derived. Both tile
//! axes run *down* the screen, `+x` to the right and `+y` to the left. So the
//! screen cardinals are tile diagonals, and screen depth is `x + y`.
//!
//! `Vec2` is `game`'s re-export, so this crate never picks its own `glam`.

use game::{Vec2, WorldVec};
use godot::builtin::Vector2;

use crate::origin::Origin;

/// How much of the isometric foreshortening to cancel out of walking speed.
///
/// The 2:1 diamond means the two cannot both be constant, so this picks where
/// on that scale to sit:
///
/// - `0.0` holds *world* speed constant. Every direction crosses the same
///   number of tiles per second, and sideways covers twice the screen pixels.
///   This is what Diablo II does, and it is the honest projection.
/// - `1.0` holds *screen* speed constant. Every direction covers the same
///   pixels per second, and walking up the screen crosses twice the tiles.
/// - Anything between splits the difference.
pub const ISO_SPEED_COMPENSATION: f32 = 0.5;

/// One tile's diamond, in pixels.
pub const TILE_WIDTH: f32 = 192.0;
pub const TILE_HEIGHT: f32 = 96.0;

/// Screen pixels per height step.
///
/// One quarter of the tile's screen height, which is the same ratio OpenTTD uses
/// against its own tile. Belongs here and not in `worldgen`: a pixel is an art
/// fact, and that crate is the one with no pixels in it.
pub const HEIGHT_STEP: f32 = TILE_HEIGHT / 4.0;

/// Where the center of `tile` sits on screen, relative to `origin`.
///
/// The subtraction happens in `f64` and the narrowing to `Vector2` is the last
/// operation. That order is the whole point: doing it the other way round would
/// throw away the precision the wider world position exists to keep.
#[must_use]
pub fn tile_to_screen(tile: WorldVec, origin: Origin) -> Vector2 {
    ground_to_screen(tile, 0, origin)
}

/// Where a point at `height` steps above the ground sits on screen.
///
/// Height moves things up the screen rather than into a sort key. Godot y sorts
/// terrain by cell and entities by node position, and inside a y sorted parent a
/// `z_index` overrides the sort rather than refining it, so raising the drawn
/// position is the mechanism. Cliff tiles then carry a per tile `y_sort_origin`
/// so they still sort by their base.
#[must_use]
pub fn ground_to_screen(tile: WorldVec, height: i8, origin: Origin) -> Vector2 {
    let local = tile - origin.tile();
    let half_width = f64::from(TILE_WIDTH) / 2.0;
    let half_height = f64::from(TILE_HEIGHT) / 2.0;
    Vector2::new(
        ((local.x - local.y) * half_width + half_width) as f32,
        ((local.x + local.y) * half_height + half_height) as f32 - f32::from(height) * HEIGHT_STEP,
    )
}

/// Where a chunk's own layer node sits on screen.
///
/// A `TileMapLayer` holds chunk local cell coordinates, so the node itself
/// carries the chunk's offset. One function for it, because the position is
/// written twice, once when the chunk is painted and again whenever the origin
/// rebases, and the two must not be able to disagree.
#[must_use]
pub fn chunk_to_screen(coord: worldgen::ChunkCoord, origin: Origin) -> Vector2 {
    let tile = coord.origin();
    tile_to_screen(WorldVec::new(f64::from(tile.x), f64::from(tile.y)), origin)
}

/// Undoes the projection for a *direction*, which is why `W` means up the
/// screen. Zero in gives zero out.
///
/// The one inverse in the frontend, and it serves both the movement keys and
/// the cursor. Two of them drift: the keys were once converted by two different
/// sums, so fixing one left the other wrong.
///
/// Do not remove the normalizing. It is what makes movement isotropic: the raw
/// inverse is anisotropic by exactly 2x, because `W` maps to magnitude
/// `sqrt(2)/96` and `D` to `sqrt(2)/192`.
#[must_use]
pub fn screen_dir_to_tile(screen: Vector2) -> Vec2 {
    compensated(screen, ISO_SPEED_COMPENSATION)
}

/// [`screen_dir_to_tile`] with the trade made explicit, so a test can pin both
/// ends of it. Never longer than unit length, whatever the compensation, because
/// `game::Input` clamps anything longer and that would silently undo this.
#[must_use]
pub fn compensated(screen: Vector2, compensation: f32) -> Vec2 {
    let screen = screen.normalized_or_zero();
    if screen == Vector2::ZERO {
        return Vec2::ZERO;
    }
    let x = screen.x / TILE_WIDTH; // screen x in tile widths
    let y = screen.y / TILE_HEIGHT; // screen y in tile heights
    let tile = Vec2::new(x + y, y - x).normalize();
    // Pixels one tile unit covers this way, against the cheapest direction:
    // 1.0 up the screen, 2.0 sideways, because the diamond is 2:1.
    let cost = std::f32::consts::SQRT_2 * (tile.x - tile.y).abs().hypot((tile.x + tile.y) * 0.5);
    tile / cost.powf(compensation)
}
