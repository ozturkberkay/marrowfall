use std::f32::consts::FRAC_1_SQRT_2;

use game::Vec2;
use godot::builtin::Vector2;
use render::iso;

#[test]
fn the_origin_tile_draws_at_its_own_centre() {
    assert_eq!(iso::tile_to_screen(Vec2::ZERO), Vector2::new(96.0, 48.0));
}

/// Both tile axes run down the screen, `+x` to the right and `+y` to the left,
/// which is the fact every row and sort decision downstream rests on.
#[test]
fn one_step_along_each_tile_axis_runs_down_the_screen() {
    assert_eq!(
        iso::tile_to_screen(Vec2::new(1.0, 0.0)),
        Vector2::new(192.0, 96.0)
    );
    assert_eq!(
        iso::tile_to_screen(Vec2::new(0.0, 1.0)),
        Vector2::new(0.0, 96.0)
    );
}

/// Every key combination: the screen direction `Input.get_vector` reports, and
/// the tile direction it has to become. The tile column is written as the
/// integer ratio the projection produces and normalised here, so no rounded
/// literal can hide a wrong answer.
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
/// is anisotropic by exactly 2x, so `W` would walk twice as fast as `D`.
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
