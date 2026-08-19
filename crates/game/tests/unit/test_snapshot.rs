use game::{EntityView, Facing, Locomotion, Vec2};

#[test]
fn lerp_blends_between_the_two_ends_of_a_tick() {
    let view = EntityView {
        id: 7,
        prev_pos: Vec2::ZERO,
        pos: Vec2::new(10.0, -4.0),
        facing: Facing::South,
        locomotion: Locomotion::Running,
    };

    assert_eq!(view.lerp(0.25), Vec2::new(2.5, -1.0));
    assert_eq!(view.lerp(0.5), Vec2::new(5.0, -2.0));
}

#[test]
fn lerp_clamps_alpha_to_this_tick() {
    let view = EntityView {
        id: 7,
        prev_pos: Vec2::new(1.0, 1.0),
        pos: Vec2::new(5.0, 1.0),
        facing: Facing::South,
        locomotion: Locomotion::Running,
    };

    assert_eq!(view.lerp(2.0), view.pos);
    assert_eq!(view.lerp(-1.0), view.prev_pos);
}
