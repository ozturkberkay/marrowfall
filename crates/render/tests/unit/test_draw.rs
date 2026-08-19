use game::{EntityView, Facing, Locomotion, Vec2};
use godot::builtin::{Rect2, Vector2};
use render::draw::{self, Clip};
use sprites::{Anchor, AnimationAtlas, FrameRect};

fn atlas() -> AnimationAtlas {
    AnimationAtlas {
        file: "idle.png".to_owned(),
        directions: vec!["s".to_owned(), "e".to_owned()],
        frames: 1,
        fps: 8,
        loops: true,
        cell_width: 10,
        cell_height: 20,
        anchor: Anchor { x: 5, y: 19 },
        rects: vec![
            FrameRect {
                x: 0,
                y: 0,
                w: 4,
                h: 8,
                off_x: 1,
                off_y: 2,
            },
            FrameRect {
                x: 6,
                y: 0,
                w: 4,
                h: 8,
                off_x: 1,
                off_y: 2,
            },
        ],
    }
}

fn view(id: u64) -> EntityView {
    EntityView {
        id,
        pos: Vec2::ZERO,
        prev_pos: Vec2::ZERO,
        facing: Facing::South,
        locomotion: Locomotion::Idle,
    }
}

#[test]
fn each_locomotion_picks_its_clip() {
    assert_eq!(Clip::for_locomotion(Locomotion::Idle).name(), "idle");
    assert_eq!(Clip::for_locomotion(Locomotion::Running).name(), "run");
}

/// `ALL` is what startup preloads a texture for, so a clip missing from it can
/// never be drawn.
#[test]
fn every_clip_is_preloadable() {
    for locomotion in [Locomotion::Idle, Locomotion::Running] {
        let clip = Clip::for_locomotion(locomotion);
        assert!(Clip::ALL.contains(&clip), "{clip:?} is not in ALL");
    }
}

#[test]
fn a_frame_draws_where_its_pixels_sat_inside_the_cell() {
    let rect = FrameRect {
        x: 40,
        y: 12,
        w: 4,
        h: 8,
        off_x: 3,
        off_y: 7,
    };

    let placed = draw::placement(&atlas(), &rect);

    assert_eq!(
        placed.region,
        Rect2::new(Vector2::new(40.0, 12.0), Vector2::new(4.0, 8.0))
    );
    // The cell's top left sits at minus the anchor, and the frame sits at its
    // own offset inside that cell, so the anchor lands on the node origin.
    assert_eq!(placed.offset, Vector2::new(3.0 - 5.0, 7.0 - 19.0));
}

#[test]
fn an_entity_with_no_node_yet_is_added() {
    let changes = draw::reconcile(&[view(7), view(9)], [7]);

    assert_eq!(changes.added, [9]);
    assert!(changes.removed.is_empty());
}

/// Absent from a snapshot means despawned, because every live entity is in
/// every snapshot.
#[test]
fn an_entity_absent_from_the_snapshot_is_removed() {
    let changes = draw::reconcile(&[view(7)], [7, 9]);

    assert!(changes.added.is_empty());
    assert_eq!(changes.removed, [9]);
}

#[test]
fn a_snapshot_that_changed_nothing_asks_for_nothing() {
    let changes = draw::reconcile(&[view(7), view(9)], [9, 7]);

    assert!(changes.added.is_empty(), "{:?}", changes.added);
    assert!(changes.removed.is_empty(), "{:?}", changes.removed);
}

/// hecs packs a generation into the id, so a recycled entity slot comes back as
/// a different `u64`. It must free the old node and make a new one rather than
/// quietly inherit one.
#[test]
fn a_recycled_slot_frees_the_old_node_and_makes_a_new_one() {
    let second_generation = (1 << 32) | 7;

    let changes = draw::reconcile(&[view(second_generation)], [7]);

    assert_eq!(changes.added, [second_generation]);
    assert_eq!(changes.removed, [7]);
}
