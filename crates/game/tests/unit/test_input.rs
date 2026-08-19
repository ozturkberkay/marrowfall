use game::{Input, Vec2};

#[test]
fn held_input_passes_a_unit_direction_through_untouched() {
    let held = Vec2::new(0.6, -0.8);
    assert_eq!(Input::new(held).move_dir(), held);
}

#[test]
fn no_input_is_the_default() {
    assert_eq!(Input::default().move_dir(), Vec2::ZERO);
}

/// The trust boundary against a malformed frontend: a longer vector would move
/// the player faster than `PLAYER_SPEED`.
#[test]
fn a_longer_than_unit_direction_is_scaled_back() {
    let scaled = Input::new(Vec2::new(30.0, -40.0)).move_dir();
    assert!(
        scaled.abs_diff_eq(Vec2::new(0.6, -0.8), 1e-6),
        "50 units long became {scaled}"
    );
}

/// A non-finite position spreads through every later tick and makes a snapshot
/// unequal to itself, which would break any replay comparison.
#[test]
fn a_non_finite_direction_becomes_still() {
    for broken in [
        Vec2::new(f32::NAN, 0.0),
        Vec2::new(0.0, f32::INFINITY),
        Vec2::splat(f32::NEG_INFINITY),
    ] {
        assert_eq!(
            Input::new(broken).move_dir(),
            Vec2::ZERO,
            "{broken} should not move anything"
        );
    }
}
