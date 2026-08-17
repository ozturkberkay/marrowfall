use game::{EntityView, RenderSnapshot};
use glam::Vec2;

fn snapshot(tick: u64, entities: Vec<EntityView>) -> RenderSnapshot {
    RenderSnapshot {
        tick,
        time: 0.0,
        entities,
    }
}

#[test]
fn lerp_blends_positions_by_alpha() {
    let prev = snapshot(
        1,
        vec![EntityView {
            id: 7,
            pos: Vec2::ZERO,
        }],
    );
    let curr = snapshot(
        2,
        vec![EntityView {
            id: 7,
            pos: Vec2::new(10.0, -4.0),
        }],
    );

    let mid = RenderSnapshot::lerp(&prev, &curr, 0.5);
    assert_eq!(
        mid,
        vec![EntityView {
            id: 7,
            pos: Vec2::new(5.0, -2.0),
        }]
    );
}

#[test]
fn newly_spawned_entities_snap_to_current_position() {
    let prev = snapshot(1, vec![]);
    let curr = snapshot(
        2,
        vec![EntityView {
            id: 9,
            pos: Vec2::new(3.0, 3.0),
        }],
    );

    let out = RenderSnapshot::lerp(&prev, &curr, 0.25);
    assert_eq!(out, curr.entities);
}

#[test]
fn entities_missing_from_current_are_dropped() {
    let prev = snapshot(
        1,
        vec![EntityView {
            id: 1,
            pos: Vec2::ZERO,
        }],
    );
    let curr = snapshot(2, vec![]);

    assert!(RenderSnapshot::lerp(&prev, &curr, 0.5).is_empty());
}
