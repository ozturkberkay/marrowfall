use std::f32::consts::{FRAC_PI_4, TAU};

use game::{EntityView, Facing, Locomotion, Vec2, WorldVec};
use godot::builtin::{Rect2, Vector2};
use render::draw::{self, Clip, GAIT_SECONDS};
use sprites::{Anchor, AnimationAtlas, FrameRect};

/// One atlas of the shape the packer writes: `rows` directions by `frames`
/// frames. Rows are numbered rather than named, because nothing here reads a
/// row's name; which direction each row holds is pinned in `crates/sprites`
/// against the shipped manifest.
fn atlas_of(rows: usize, frames: u32) -> AnimationAtlas {
    AnimationAtlas {
        file: "idle.png".to_owned(),
        directions: (0..rows).map(|row| row.to_string()).collect(),
        frames,
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
            };
            rows * frames as usize
        ],
    }
}

fn view(id: u64) -> EntityView {
    EntityView {
        id,
        pos: WorldVec::ZERO,
        prev_pos: WorldVec::ZERO,
        facing: Facing::South,
        locomotion: Locomotion::Idle,
        aim: Facing::South.axis(),
        height: 0,
    }
}

/// Every simulation state, the clip it draws as, and whether that clip is a
/// stride. In `Clip::ALL` order, so one table pins the preload list as well: a
/// clip missing from `ALL` has no texture and can never be drawn.
const CLIPS: [(Locomotion, &str, bool); 5] = [
    (Locomotion::Idle, "idle", false),
    (Locomotion::Forward, "run", true),
    (Locomotion::Backward, "walk_back", true),
    (Locomotion::StrafeLeft, "strafe_left", true),
    (Locomotion::StrafeRight, "strafe_right", true),
];

#[test]
fn each_locomotion_picks_its_clip() {
    let mut asked = Vec::new();
    for (locomotion, name, stride) in CLIPS {
        let clip = Clip::for_locomotion(locomotion);
        assert_eq!(clip.name(), name);
        assert_eq!(clip.is_locomotion(), stride, "is {name} a stride?");
        asked.push(clip);
    }
    assert_eq!(Clip::ALL.as_slice(), asked, "ALL is what startup preloads");
}

/// `strafe_left` and `strafe_right` have no art yet, so those strides borrow the
/// run. His feet then push the wrong way, which reads far better than a body
/// frozen mid stride, and it keeps the game playable until the clips land.
#[test]
fn a_stride_with_no_art_yet_borrows_the_run() {
    let baked = |clip| matches!(clip, Clip::Idle | Clip::Run | Clip::WalkBack);

    assert_eq!(Clip::StrafeLeft.with_art(baked), Some(Clip::Run));
    assert_eq!(Clip::StrafeRight.with_art(baked), Some(Clip::Run));
    assert_eq!(Clip::WalkBack.with_art(baked), Some(Clip::WalkBack));
}

/// Nothing stands in for `idle`, or for the run itself, so a manifest missing
/// either leaves the sprite alone instead of showing the wrong body.
#[test]
fn a_clip_with_no_stand_in_draws_nothing() {
    for clip in Clip::ALL {
        assert_eq!(clip.with_art(|_| false), None, "{clip:?}");
    }
    assert_eq!(Clip::Idle.with_art(|clip| clip == Clip::Run), None);
}

/// A viewport the size the project asks for. Its own coordinates, so the window
/// it is stretched into makes no difference.
const VIEWPORT: Vector2 = Vector2::new(2560.0, 1440.0);

/// The camera pins his *feet* to the middle of the viewport, so the dead zone
/// has to be lifted onto his body. Measured from the middle of the screen, the
/// cursor on his chest would read as a hard push north.
#[test]
fn the_cursor_on_the_middle_of_his_body_asks_for_nothing() {
    let body = VIEWPORT / 2.0 - Vector2::new(0.0, draw::BODY_MIDDLE);

    assert_eq!(draw::cursor_offset(body, VIEWPORT), Vector2::ZERO);
    for edge in [Vector2::new(0.0, 1.0), Vector2::new(-1.0, 0.0)] {
        let inside = body + edge * (draw::DEAD_RADIUS - 1.0);
        assert_eq!(
            draw::cursor_offset(inside, VIEWPORT),
            Vector2::ZERO,
            "{edge}"
        );
    }
}

/// Outside the dead zone the offset is measured from his body, and his feet at
/// the middle of the screen are one half-height below it.
#[test]
fn the_cursor_off_his_body_is_measured_from_his_body() {
    let above = VIEWPORT / 2.0 - Vector2::new(0.0, draw::BODY_MIDDLE + 200.0);
    assert_eq!(
        draw::cursor_offset(above, VIEWPORT),
        Vector2::new(0.0, -200.0)
    );

    let feet = draw::cursor_offset(VIEWPORT / 2.0, VIEWPORT);
    assert_eq!(feet, Vector2::new(0.0, draw::BODY_MIDDLE));
}

/// The bake's ring, clockwise on screen from south, and the tile direction each
/// stop points. `Facing::axis` is the independent source: a mirrored ring swaps
/// east with west and leaves south and north right, so half the rows still look
/// correct.
const RING: [Facing; 8] = [
    Facing::South,
    Facing::SouthWest,
    Facing::West,
    Facing::NorthWest,
    Facing::North,
    Facing::NorthEast,
    Facing::East,
    Facing::SouthEast,
];

/// Row 0 is south, which is tile `(1, 1)`: the bake starts at the model facing
/// the camera and turns from there.
#[test]
fn row_zero_is_south() {
    let row = draw::row_for_aim(&atlas_of(16, 1), Vec2::new(1.0, 1.0));
    assert_eq!(row, Some(0));
}

/// A finer ring holds the coarse one's directions, `count / 8` rows apart. That
/// is what makes changing the pose count a re-bake and no code change.
#[test]
fn every_ring_puts_the_same_directions_in_the_same_places() {
    for count in [8usize, 16, 32] {
        let atlas = atlas_of(count, 1);
        for (stop, facing) in RING.into_iter().enumerate() {
            assert_eq!(
                draw::row_for_aim(&atlas, facing.axis()),
                Some(stop * count / 8),
                "{facing:?} at {count} rows"
            );
        }
    }
}

/// Sweeping the aim once round visits every row exactly once, in bake order, or
/// some pose is unreachable and the atlas carries a row nothing ever draws.
#[test]
fn sweeping_the_aim_visits_every_row_in_bake_order() {
    for count in [8usize, 16, 32] {
        let atlas = atlas_of(count, 1);
        let wedge = TAU / count as f32;
        let swept: Vec<_> = (0..count)
            .map(|row| draw::row_for_aim(&atlas, Vec2::from_angle(FRAC_PI_4 + row as f32 * wedge)))
            .collect();

        let want: Vec<_> = (0..count).map(Some).collect();
        assert_eq!(swept, want, "{count} rows");
    }
}

/// A zero aim is the dead radius around the survivor, and a rowless atlas
/// cannot come out of `sprites::parse` but the fields are public. Both leave the
/// sprite on the row it had.
#[test]
fn an_aim_or_an_atlas_with_no_direction_has_no_row() {
    assert_eq!(draw::row_for_aim(&atlas_of(16, 1), Vec2::ZERO), None);
    assert_eq!(
        draw::row_for_aim(&atlas_of(0, 1), Vec2::new(1.0, 1.0)),
        None
    );
}

/// The shared cycle is `run`'s own, so the clip every other stride is stretched
/// onto keeps its authored rate. Pinned against the shipped manifest, because a
/// re-authored run would otherwise drift away from the constant.
#[test]
fn the_shared_cycle_is_the_shipped_runs_own_length() {
    const SURVIVOR: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../project/assets/characters/survivor/character.ron"
    ));

    let assets = sprites::parse(SURVIVOR).unwrap();
    let run = assets.animations.get("run").unwrap();
    let cycle = f64::from(run.frames) / f64::from(run.fps);

    assert!(
        (GAIT_SECONDS - cycle).abs() < 1e-12,
        "the shipped run's cycle is {cycle}s, not {GAIT_SECONDS}s"
    );
}

#[test]
fn a_stride_starts_on_its_first_frame() {
    let run = atlas_of(1, 20);
    assert_eq!(draw::locomotion_frame(&run, 0.0), 0);
    // The seed snapshot stamps time 0, and the frontend walks back one tick
    // from whatever it is given.
    assert_eq!(draw::locomotion_frame(&run, -1.0), 0);
}

#[test]
fn a_stride_wraps_at_the_shared_cycle() {
    let run = atlas_of(1, 20);
    assert_eq!(draw::locomotion_frame(&run, GAIT_SECONDS * 0.5), 10);
    assert_eq!(draw::locomotion_frame(&run, GAIT_SECONDS), 0);
    assert_eq!(draw::locomotion_frame(&run, GAIT_SECONDS * 4.0), 0);
}

/// `run` is 20 frames and `walk_back` is 18, so the same instant is a different
/// frame in each. What has to match is the fraction through the stride: that is
/// what stops the legs jumping when the stride changes.
#[test]
fn clips_of_different_lengths_stay_in_phase() {
    let run = atlas_of(1, 20);
    let back = atlas_of(1, 18);

    for step in 0..20 {
        let seconds = GAIT_SECONDS * f64::from(step) / 20.0;
        let running = draw::locomotion_frame(&run, seconds) as f64 / 20.0;
        let backing = draw::locomotion_frame(&back, seconds) as f64 / 18.0;
        assert!(
            (running - backing).abs() <= 1.0 / 18.0,
            "at {seconds}s the run is {running} through its cycle and the walk back {backing}"
        );
    }
}

/// `sprites::parse` rejects a frameless atlas, but the fields are public, and a
/// phase of exactly one would index past the last frame.
#[test]
fn no_instant_ever_leaves_the_atlas() {
    for frames in [0u32, 1, 18, 20] {
        let atlas = atlas_of(1, frames);
        for step in 0..=96 {
            let seconds = f64::from(step) * GAIT_SECONDS / 96.0;
            let frame = draw::locomotion_frame(&atlas, seconds);
            assert!(
                frame < frames.max(1) as usize,
                "{frames} frames at {seconds}s gave frame {frame}"
            );
        }
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

    let placed = draw::placement(&atlas_of(2, 1), &rect);

    assert_eq!(
        placed.region,
        Rect2::new(Vector2::new(40.0, 12.0), Vector2::new(4.0, 8.0))
    );
    // The cell's top left sits at minus the anchor, and the frame sits at its
    // own offset inside that cell. So the anchor lands on the node origin.
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
/// a different `u64`. The frontend must free the old node and make a new one,
/// not inherit the old one.
#[test]
fn a_recycled_slot_frees_the_old_node_and_makes_a_new_one() {
    let second_generation = (1 << 32) | 7;

    let changes = draw::reconcile(&[view(second_generation)], [7]);

    assert_eq!(changes.added, [second_generation]);
    assert_eq!(changes.removed, [7]);
}
