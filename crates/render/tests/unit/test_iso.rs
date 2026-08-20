use std::f32::consts::{FRAC_1_SQRT_2, SQRT_2};

use game::{Vec2, WorldVec};
use godot::builtin::Vector2;
use render::iso::{self, TILE_HEIGHT};
use render::origin::Origin;
use worldgen::ChunkCoord;

/// An origin on the world origin, so these assertions read as absolute pixels.
fn home() -> Origin {
    Origin::at(WorldVec::ZERO)
}

/// Screen pixels a one tile direction covers. The origin cancels, so any will
/// do.
fn pixels(tile: Vec2) -> f32 {
    (iso::tile_to_screen(tile.as_dvec2(), home()) - iso::tile_to_screen(WorldVec::ZERO, home()))
        .length()
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
        let want = ratio.normalize() * tile.length();
        assert!(
            tile.abs_diff_eq(want, 1e-6),
            "{keys} became {tile}, not {want}"
        );
    }
}

/// No keys held is no request, which the simulation reads as standing still.
#[test]
fn no_keys_held_asks_for_no_movement() {
    assert_eq!(iso::screen_dir_to_tile(Vector2::ZERO), Vec2::ZERO);
}

/// The compensation trades length only. If it ever turned the direction as
/// well, `W` would stop meaning up the screen.
#[test]
fn the_compensation_changes_the_length_and_never_the_direction() {
    for (keys, screen, _) in key_combinations() {
        let honest = iso::compensated(screen, 0.0);
        for step in 0..=10 {
            let compensation = step as f32 / 10.0;
            let tile = iso::compensated(screen, compensation).normalize();
            assert!(
                tile.abs_diff_eq(honest, 1e-6),
                "{keys} at {compensation} points {tile}, not {honest}"
            );
        }
    }
}

/// The 2:1 diamond means world speed and screen speed cannot both be constant.
/// These pin both ends of `ISO_SPEED_COMPENSATION` so the trade stays a choice
/// rather than an accident.
#[test]
fn no_compensation_holds_world_speed_constant() {
    for (keys, screen, _) in key_combinations() {
        let length = iso::compensated(screen, 0.0).length();
        assert!(
            (length - 1.0).abs() < 1e-6,
            "{keys} asked for {length} tiles"
        );
    }
}

#[test]
fn full_compensation_holds_screen_speed_constant() {
    let want = TILE_HEIGHT / std::f32::consts::SQRT_2;
    for (keys, screen, _) in key_combinations() {
        let covered = pixels(iso::compensated(screen, 1.0));
        assert!(
            (covered - want).abs() < 1e-3,
            "{keys} covered {covered} px, not {want}"
        );
    }
}

/// The shipped setting, and the two numbers it promises: sideways covers about
/// 1.41 times the pixels of up-screen instead of 2.0, and crosses about 0.71 of
/// the tiles instead of 0.5. Neither failure is large, which is the point.
#[test]
fn half_compensation_splits_the_difference() {
    let sideways = iso::compensated(Vector2::new(1.0, 0.0), 0.5);
    let up_screen = iso::compensated(Vector2::new(0.0, -1.0), 0.5);

    let tiles = sideways.length() / up_screen.length();
    assert!(
        (tiles - FRAC_1_SQRT_2).abs() < 1e-6,
        "sideways crossed {tiles} of the tiles up-screen crosses"
    );

    let ratio = pixels(sideways) / pixels(up_screen);
    assert!(
        (ratio - SQRT_2).abs() < 1e-6,
        "sideways covered {ratio} times the pixels of up-screen"
    );
}

/// Whatever the compensation, the direction must never exceed unit length:
/// `game::Input` clamps beyond that, which would silently reintroduce the
/// anisotropy this exists to control.
#[test]
fn no_compensation_setting_ever_exceeds_the_input_clamp() {
    for step in 0..=10 {
        let compensation = step as f32 / 10.0;
        for (keys, screen, _) in key_combinations() {
            let length = iso::compensated(screen, compensation).length();
            assert!(
                length <= 1.0 + 1e-6,
                "{keys} at {compensation} asked for {length}"
            );
        }
    }
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

#[test]
fn a_chunk_lands_where_its_own_origin_tile_lands() {
    // The layer holds chunk local cells, so the node has to sit exactly where
    // tile (0,0) of that chunk would be drawn.
    let origin = Origin::default();
    for coord in [
        ChunkCoord::new(0, 0),
        ChunkCoord::new(3, -2),
        ChunkCoord::new(-7, 11),
    ] {
        let tile = coord.origin();
        let expected =
            iso::tile_to_screen(WorldVec::new(f64::from(tile.x), f64::from(tile.y)), origin);
        assert_eq!(iso::chunk_to_screen(coord, origin), expected, "{coord:?}");
    }
}

#[test]
fn rebasing_shifts_every_chunk_by_the_same_amount() {
    // What the rebase relies on: the origin moving changes all chunk positions by
    // one common offset, so terrain and entities stay aligned with each other.
    let before = Origin::default();
    let mut after = Origin::default();
    assert!(
        after.follow(WorldVec::new(500.0, 500.0)),
        "expected a rebase"
    );

    let shift = |c| iso::chunk_to_screen(c, after) - iso::chunk_to_screen(c, before);
    let first = shift(ChunkCoord::new(0, 0));
    for coord in [ChunkCoord::new(4, 1), ChunkCoord::new(-3, 6)] {
        assert_eq!(shift(coord), first, "{coord:?} shifted differently");
    }
    assert_ne!(first, Vector2::ZERO, "the rebase moved nothing");
}
