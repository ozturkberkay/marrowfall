//! The isometric mapping between tile space and screen space, in one place.
//!
//! Measured against Godot's isometric `TileMapLayer` rather than derived. Both
//! tile axes run *down* the screen, `+x` to the right and `+y` to the left, so
//! the screen cardinals are tile diagonals and screen depth is `x + y`.
//!
//! `Vec2` is `game`'s re-export, so this crate never picks its own `glam`.

use game::Vec2;
use godot::builtin::Vector2;

/// One tile's diamond, in pixels.
pub const TILE_WIDTH: f32 = 192.0;
pub const TILE_HEIGHT: f32 = 96.0;

/// Where the centre of `tile` sits on screen.
#[must_use]
pub fn tile_to_screen(tile: Vec2) -> Vector2 {
    let half_width = TILE_WIDTH / 2.0;
    let half_height = TILE_HEIGHT / 2.0;
    Vector2::new(
        (tile.x - tile.y) * half_width + half_width,
        (tile.x + tile.y) * half_height + half_height,
    )
}

/// Undoes the projection for a *direction*, which is why `W` means up the
/// screen. Zero in gives zero out.
///
/// The `normalize_or_zero` is what makes movement isotropic and must not be
/// removed: the inverse is anisotropic by exactly 2x, since `W` maps to
/// magnitude `sqrt(2)/96` and `D` to `sqrt(2)/192`.
#[must_use]
pub fn screen_dir_to_tile(screen: Vector2) -> Vec2 {
    let x = screen.x / TILE_WIDTH; // screen x in tile widths
    let y = screen.y / TILE_HEIGHT; // screen y in tile heights
    Vec2::new(x + y, y - x).normalize_or_zero()
}
