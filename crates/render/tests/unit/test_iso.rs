use std::f32::consts::FRAC_1_SQRT_2;

use game::{Vec2, WorldVec};
use godot::builtin::Vector2;
use render::iso;
use render::origin::Origin;

/// An origin on the world origin, so these assertions read as absolute pixels.
fn home() -> Origin {
    Origin::at(WorldVec::ZERO)
}

#[test]
fn the_origin_tile_draws_at_its_own_centre() {
    assert_eq!(
        iso::tile_to_screen(WorldVec::ZERO, home()),
        Vector2::new(96.0, 48.0)
    );
}

/// Both tile axes run down the screen, `+x` to the right and `+y` to the left.
/// Every row and sort decision downstream rests on this fact.
#[test]
fn one_step_along_each_tile_axis_runs_down_the_screen() {
    assert_eq!(
        iso::tile_to_screen(WorldVec::new(1.0, 0.0), home()),
        Vector2::new(192.0, 96.0)
    );
    assert_eq!(
        iso::tile_to_screen(WorldVec::new(0.0, 1.0), home()),
        Vector2::new(0.0, 96.0)
    );
}

/// Every key combination: the screen direction `Input.get_vector` reports, and
/// the tile direction it has to become. The tile column holds the integer ratio
/// the projection produces, normalised here, so no rounded literal hides a
/// wrong answer.
fn key_combinations() -> [(&'static str, Vector2, Vec2); 8] {
    let d = FRAC_1_SQRT_2;
    [
        ("W", Vector2::new(0.0, -1.0), Vec2::new(-1.0, -1.0)),
        ("W+D", Vector2::new(d, -d), Vec2::new(-1.0, -3.0)),
        ("D", Vector2::new(1.0, 0.0), Vec2::new(1.0, -1.0)),
        ("S+D", Vector2::new(d, d), Vec2::new(3.0, 1.0)),
        ("S", Vector2::new(0.0, 1.0), Vec2::new(1.0, 1.0)),
        ("S+A", Vector2::new(-d, d), Vec2::new(1.0, 3.0)),
        ("A", Vector2::new(-1.0, 0.0), Vec2::new(-1.0, 1.0)),
        ("W+A", Vector2::new(-d, -d), Vec2::new(-3.0, -1.0)),
    ]
}

#[test]
fn every_key_combination_points_the_way_it_looks_on_screen() {
    for (keys, screen, ratio) in key_combinations() {
        let tile = iso::screen_dir_to_tile(screen);
        let want = ratio.normalize();
        assert!(
            tile.abs_diff_eq(want, 1e-6),
            "{keys} became {tile}, not {want}"
        );
    }
}

/// A unit direction in, a unit direction out. Without the normalise the inverse
/// is anisotropic by exactly 2x, and `W` walks twice as fast as `D`.
#[test]
fn every_key_combination_is_the_same_speed() {
    for (keys, screen, _) in key_combinations() {
        let length = iso::screen_dir_to_tile(screen).length();
        assert!((length - 1.0).abs() < 1e-6, "{keys} asked for {length}");
    }
}

#[test]
fn no_keys_held_is_no_direction() {
    assert_eq!(iso::screen_dir_to_tile(Vector2::ZERO), Vec2::ZERO);
}

/// The whole reason the origin exists. At 30 km out an `f32` screen coordinate
/// can no longer resolve a quarter pixel, so a rebased origin is what keeps the
/// numbers small enough to place a sprite exactly.
#[test]
fn a_tile_far_from_the_world_origin_still_lands_on_an_exact_pixel() {
    let far = WorldVec::new(30_000.0, 30_000.0);
    let origin = Origin::at(far);
    let screen = iso::tile_to_screen(far, origin);
    // Within one chunk of the origin, so the coordinates stay in the thousands
    // however far out the tile is.
    assert!(
        screen.x.abs() < 10_000.0 && screen.y.abs() < 10_000.0,
        "{screen} is too large to place precisely"
    );
    // And a one tile step is still exactly one tile, which is what an absolute
    // f32 coordinate loses at this distance.
    let next = iso::tile_to_screen(far + WorldVec::new(1.0, 0.0), origin);
    assert_eq!(next - screen, Vector2::new(96.0, 48.0));
}

#[test]
fn the_origin_only_moves_once_the_player_leaves_its_slack() {
    let mut origin = Origin::at(WorldVec::ZERO);
    let start = origin;
    // Inside the slack: no rebase, so nothing already drawn has to move.
    assert!(!origin.follow(WorldVec::new(10.0, 10.0)));
    assert_eq!(origin, start);
    // Well outside it: rebase.
    assert!(origin.follow(WorldVec::new(5_000.0, -5_000.0)));
    assert_ne!(origin, start);
}

#[test]
fn an_origin_sits_on_a_chunk_corner() {
    // Whole chunks, so a rebase moves the world by an exact number of tiles and
    // cannot introduce a sub-tile offset of its own.
    let chunk = f64::from(worldgen::CHUNK_TILES);
    for tile in [
        WorldVec::new(0.0, 0.0),
        WorldVec::new(31.9, 31.9),
        WorldVec::new(-1.0, -1.0),
        WorldVec::new(9_999.5, -9_999.5),
    ] {
        let at = Origin::at(tile).tile();
        assert_eq!(at.x % chunk, 0.0, "{at} is not on a chunk corner");
        assert_eq!(at.y % chunk, 0.0, "{at} is not on a chunk corner");
    }
}
