use game::{
    EntityView, Facing, Input, Locomotion, PLAYER_SPEED, RenderSnapshot, Sim, Spawn, TICK_HZ, Vec2,
};

const SEED: u64 = 0x4D61_7272_6F77; // "Marrow"

/// Deliberately awkward magnitudes on both axes, so no assertion can lean on
/// a velocity that happens to round cleanly.
const DRIFT: Vec2 = Vec2::new(3.0, -7.0);

/// One axis each. Named for the axis, not a screen direction: which way `+y`
/// points on screen is the frontend's business, not the simulation's.
const ALONG_X: Vec2 = Vec2::new(3.0, 0.0);
const ALONG_Y: Vec2 = Vec2::new(0.0, -7.0);

/// Far enough inside the field that a second of walking cannot reach an edge,
/// so a test about speed is not also a test about the clamp.
const MIDFIELD: Vec2 = Vec2::new(8.0, 8.0);

/// A world holding exactly these entities, with their ids positionally, so a
/// caller can destructure them by name.
fn world_of<const N: usize>(spawns: [Spawn; N]) -> (Sim, [u64; N]) {
    let (sim, ids) = Sim::with_entities(SEED, &spawns);
    (sim, ids.try_into().expect("one id per spawn"))
}

fn moving(at: Vec2, velocity: Vec2) -> Spawn {
    Spawn {
        at,
        velocity: Some(velocity),
        player: false,
    }
}

/// The one entity held input drives. It starts still, because input is the
/// only thing that ever gives it a velocity.
fn player(at: Vec2) -> Spawn {
    Spawn {
        at,
        velocity: Some(Vec2::ZERO),
        player: true,
    }
}

fn only_entity(snapshot: &RenderSnapshot) -> EntityView {
    assert_eq!(snapshot.entities.len(), 1);
    snapshot.entities[0]
}

fn view_of(snapshot: &RenderSnapshot, id: u64) -> EntityView {
    *snapshot
        .entities
        .iter()
        .find(|entity| entity.id == id)
        .expect("entity missing from snapshot")
}

#[test]
fn same_seed_generates_identical_terrain() {
    assert_eq!(Sim::new(SEED).terrain(), Sim::new(SEED).terrain());
}

#[test]
fn different_seeds_generate_different_terrain() {
    assert_ne!(Sim::new(SEED).terrain(), Sim::new(SEED + 1).terrain());
}

#[test]
fn the_same_world_built_the_same_way_replays_identically() {
    let run = || {
        let (mut sim, _) = world_of([
            moving(Vec2::new(1.0, 2.0), DRIFT),
            Spawn {
                at: Vec2::new(5.0, 5.0),
                velocity: None,
                player: false,
            },
        ]);
        for _ in 0..TICK_HZ {
            sim.tick(Input::default(), &[]);
        }
        sim.snapshot()
    };

    let first = run();
    assert_eq!(first.entities.len(), 2);
    assert_eq!(first, run());
}

#[test]
fn ticking_advances_time_by_fixed_steps() {
    let mut sim = Sim::new(SEED);
    for _ in 0..TICK_HZ {
        sim.tick(Input::default(), &[]);
    }
    assert_eq!(sim.ticks(), u64::from(TICK_HZ));
    assert!((sim.time() - 1.0).abs() < 1e-9);
}

#[test]
fn a_snapshot_reports_the_tick_and_time_it_describes() {
    let mut sim = Sim::new(SEED);
    for _ in 0..TICK_HZ {
        sim.tick(Input::default(), &[]);
    }

    let snapshot = sim.snapshot();
    assert_eq!(snapshot.tick, u64::from(TICK_HZ));
    assert!((snapshot.time - 1.0).abs() < 1e-9);
}

#[test]
fn snapshot_of_empty_world_has_no_entities() {
    let (mut sim, _) = world_of([]);
    sim.tick(Input::default(), &[]);
    assert!(sim.snapshot().entities.is_empty());
    assert_eq!(sim.snapshot().player, None);
}

#[test]
fn a_starting_cast_appears_in_snapshots() {
    let (sim, [id]) = world_of([moving(Vec2::new(4.0, 5.0), DRIFT)]);

    let view = only_entity(&sim.snapshot());
    assert_eq!(view.id, id);
    assert_eq!(view.pos, Vec2::new(4.0, 5.0));
}

#[test]
fn a_new_entity_has_nothing_to_interpolate() {
    let (sim, _) = world_of([moving(Vec2::new(4.0, 5.0), DRIFT)]);

    let view = only_entity(&sim.snapshot());
    assert_eq!(view.prev_pos, view.pos);
}

#[test]
fn a_tick_leaves_both_ends_of_the_move_to_interpolate() {
    let start = Vec2::new(1.0, 1.0);
    let (mut sim, _) = world_of([moving(start, DRIFT)]);

    sim.tick(Input::default(), &[]);

    let view = only_entity(&sim.snapshot());
    assert_eq!(view.prev_pos, start);
    assert!(view.pos.x > start.x, "x never advanced: {}", view.pos);
    assert!(view.pos.y < start.y, "y never advanced: {}", view.pos);
}

#[test]
fn a_second_of_ticks_moves_each_entity_by_its_own_velocity() {
    let (mut sim, [along_x, along_y]) =
        world_of([moving(Vec2::ZERO, ALONG_X), moving(Vec2::ZERO, ALONG_Y)]);

    for _ in 0..TICK_HZ {
        sim.tick(Input::default(), &[]);
    }

    let snapshot = sim.snapshot();
    let one = view_of(&snapshot, along_x).pos;
    assert!(
        one.abs_diff_eq(ALONG_X, 1e-3),
        "a second at {ALONG_X} landed at {one}"
    );
    let two = view_of(&snapshot, along_y).pos;
    assert!(
        two.abs_diff_eq(ALONG_Y, 1e-3),
        "a second at {ALONG_Y} landed at {two}"
    );
}

#[test]
fn each_tick_resumes_where_the_last_one_ended() {
    let (mut sim, _) = world_of([moving(Vec2::ZERO, DRIFT)]);

    let mut before = sim.snapshot();
    for tick in 1..=4 {
        sim.tick(Input::default(), &[]);
        let after = sim.snapshot();
        assert_eq!(
            only_entity(&after).prev_pos,
            only_entity(&before).pos,
            "tick {tick} did not resume where the previous tick ended"
        );
        before = after;
    }
}

#[test]
fn each_entity_keeps_its_own_positions() {
    let (mut sim, [along_x, along_y]) = world_of([
        moving(Vec2::ZERO, ALONG_X),
        moving(Vec2::new(9.0, 9.0), ALONG_Y),
    ]);
    assert_ne!(along_x, along_y);

    sim.tick(Input::default(), &[]);
    let after_one = sim.snapshot();
    sim.tick(Input::default(), &[]);
    let after_two = sim.snapshot();

    assert_eq!(after_two.entities.len(), 2);
    for id in [along_x, along_y] {
        assert_eq!(
            view_of(&after_two, id).prev_pos,
            view_of(&after_one, id).pos,
            "entity {id} lost its own history"
        );
    }
}

/// No `Velocity` and a zero `Velocity` are indistinguishable at the boundary,
/// and that is intended: one is skipped by integration, the other integrates
/// zero. Both must hold their ground.
#[test]
fn a_still_entity_holds_its_place_however_it_was_built() {
    for velocity in [None, Some(Vec2::ZERO)] {
        let (mut sim, _) = world_of([Spawn {
            at: Vec2::new(4.0, 4.0),
            velocity,
            player: false,
        }]);

        for _ in 0..TICK_HZ {
            sim.tick(Input::default(), &[]);
        }

        let view = only_entity(&sim.snapshot());
        assert_eq!(view.pos, Vec2::new(4.0, 4.0), "{velocity:?} drifted");
        assert_eq!(view.prev_pos, view.pos);
    }
}

/// Tile directions and the screen direction each one points, measured against
/// Godot's isometric tilemap. Both tile axes run down the screen, so the screen
/// cardinals are tile diagonals.
const FACINGS: [(Vec2, Facing, &str); 8] = [
    (Vec2::new(1.0, 1.0), Facing::South, "s"),
    (Vec2::new(1.0, 0.0), Facing::SouthEast, "se"),
    (Vec2::new(1.0, -1.0), Facing::East, "e"),
    (Vec2::new(0.0, -1.0), Facing::NorthEast, "ne"),
    (Vec2::new(-1.0, -1.0), Facing::North, "n"),
    (Vec2::new(-1.0, 0.0), Facing::NorthWest, "nw"),
    (Vec2::new(-1.0, 1.0), Facing::West, "w"),
    (Vec2::new(0.0, 1.0), Facing::SouthWest, "sw"),
];

#[test]
fn a_new_entity_faces_south() {
    let (sim, _) = world_of([moving(Vec2::ZERO, DRIFT)]);
    assert_eq!(only_entity(&sim.snapshot()).facing, Facing::South);
}

#[test]
fn facing_follows_the_direction_travelled() {
    for (velocity, want, _) in FACINGS {
        let (mut sim, _) = world_of([moving(Vec2::ZERO, velocity)]);
        sim.tick(Input::default(), &[]);
        assert_eq!(
            only_entity(&sim.snapshot()).facing,
            want,
            "travelling {velocity} should look {want:?}"
        );
    }
}

/// The two sector edges are separate comparisons, so both orders are checked:
/// tan(22.5 deg) is about 0.414, and either component can be the short one.
#[test]
fn facing_switches_sector_at_the_diagonal_boundary() {
    let cases = [
        (Vec2::new(1.0, 0.4), Facing::SouthEast),
        (Vec2::new(1.0, 0.5), Facing::South),
        (Vec2::new(0.4, 1.0), Facing::SouthWest),
        (Vec2::new(0.5, 1.0), Facing::South),
    ];
    for (velocity, want) in cases {
        let (mut sim, _) = world_of([moving(Vec2::ZERO, velocity)]);
        sim.tick(Input::default(), &[]);
        assert_eq!(
            only_entity(&sim.snapshot()).facing,
            want,
            "travelling {velocity} should look {want:?}"
        );
    }
}

#[test]
fn facing_holds_steady_while_travelling() {
    let (mut sim, _) = world_of([moving(Vec2::ZERO, Vec2::new(-1.0, -1.0))]);

    for _ in 0..TICK_HZ {
        sim.tick(Input::default(), &[]);
        assert_eq!(only_entity(&sim.snapshot()).facing, Facing::North);
    }
}

#[test]
fn a_still_entity_keeps_facing_south() {
    for velocity in [None, Some(Vec2::ZERO)] {
        let (mut sim, _) = world_of([Spawn {
            at: Vec2::new(4.0, 4.0),
            velocity,
            player: false,
        }]);
        sim.tick(Input::default(), &[]);
        assert_eq!(only_entity(&sim.snapshot()).facing, Facing::South);
    }
}

#[test]
fn each_facing_names_its_manifest_direction() {
    for (_, facing, name) in FACINGS {
        assert_eq!(
            facing.name(),
            name,
            "{facing:?} is named {name:?} in an atlas"
        );
    }
}

/// Every key combination, and the tile direction the frontend's inverse
/// projection turns it into. Rounded to two decimals from the design's table:
/// facing quantises on the ratio of the two components, and the closest of
/// these to a sector edge sits at 0.34 against the edge's 0.41, so rounding
/// cannot move one across.
const KEY_COMBINATIONS: [(&str, Vec2, Facing); 8] = [
    ("W", Vec2::new(-0.71, -0.71), Facing::North),
    ("W+D", Vec2::new(-0.32, -0.95), Facing::NorthEast),
    ("D", Vec2::new(0.71, -0.71), Facing::East),
    ("S+D", Vec2::new(0.95, 0.32), Facing::SouthEast),
    ("S", Vec2::new(0.71, 0.71), Facing::South),
    ("S+A", Vec2::new(0.32, 0.95), Facing::SouthWest),
    ("A", Vec2::new(-0.71, 0.71), Facing::West),
    ("W+A", Vec2::new(-0.95, -0.32), Facing::NorthWest),
];

#[test]
fn every_key_combination_faces_the_way_it_points_on_screen() {
    for (keys, held, want) in KEY_COMBINATIONS {
        let (mut sim, _) = world_of([player(MIDFIELD)]);
        sim.tick(Input::new(held), &[]);
        assert_eq!(
            only_entity(&sim.snapshot()).facing,
            want,
            "holding {keys} should look {want:?}"
        );
    }
}

#[test]
fn a_second_of_held_input_moves_the_player_by_the_player_speed() {
    let (mut sim, _) = world_of([player(MIDFIELD)]);

    for _ in 0..TICK_HZ {
        sim.tick(Input::new(Vec2::new(1.0, 0.0)), &[]);
    }

    let landed = only_entity(&sim.snapshot()).pos;
    let want = MIDFIELD + Vec2::new(PLAYER_SPEED, 0.0);
    assert!(
        landed.abs_diff_eq(want, 1e-3),
        "landed at {landed}, not {want}"
    );
}

/// The frontend hands over a unit direction, so a diagonal covers the same
/// ground as a cardinal rather than 1.41 times as much.
#[test]
fn a_diagonal_is_no_faster_than_a_cardinal() {
    let (mut sim, _) = world_of([player(MIDFIELD)]);

    for _ in 0..TICK_HZ {
        sim.tick(Input::new(Vec2::new(1.0, 1.0).normalize()), &[]);
    }

    let travelled = (only_entity(&sim.snapshot()).pos - MIDFIELD).length();
    assert!(
        (travelled - PLAYER_SPEED).abs() < 1e-3,
        "a diagonal second covered {travelled}, not {PLAYER_SPEED}"
    );
}

#[test]
fn input_leaves_everything_without_the_player_marker_alone() {
    let (mut sim, _) = world_of([moving(MIDFIELD, ALONG_X)]);

    for _ in 0..TICK_HZ {
        sim.tick(Input::new(Vec2::new(0.0, 1.0)), &[]);
    }

    let landed = only_entity(&sim.snapshot()).pos;
    let want = MIDFIELD + ALONG_X;
    assert!(
        landed.abs_diff_eq(want, 1e-3),
        "input overrode its own velocity: {landed}"
    );
}

#[test]
fn releasing_the_keys_stops_the_player_and_leaves_his_facing_alone() {
    let (mut sim, _) = world_of([player(MIDFIELD)]);
    let north = Vec2::new(-0.71, -0.71);

    for _ in 0..TICK_HZ / 2 {
        sim.tick(Input::new(north), &[]);
    }
    let walking = only_entity(&sim.snapshot());
    assert_eq!(walking.facing, Facing::North);
    assert_eq!(walking.locomotion, Locomotion::Running);

    for _ in 0..TICK_HZ / 2 {
        sim.tick(Input::default(), &[]);
    }
    let stopped = only_entity(&sim.snapshot());
    assert_eq!(
        stopped.pos, walking.pos,
        "he kept walking after the release"
    );
    assert_eq!(stopped.prev_pos, stopped.pos);
    assert_eq!(
        stopped.facing,
        Facing::North,
        "facing snapped back on a stop"
    );
    assert_eq!(stopped.locomotion, Locomotion::Idle);
}

/// One tile short of each edge, so a second of walking overshoots it.
fn field_edges(sim: &Sim) -> [(Vec2, Vec2, Vec2); 4] {
    let far = Vec2::new(
        (sim.terrain().width() - 1) as f32,
        (sim.terrain().height() - 1) as f32,
    );
    let mid = far / 2.0;
    [
        (
            Vec2::new(1.0, mid.y),
            Vec2::new(-1.0, 0.0),
            Vec2::new(0.0, mid.y),
        ),
        (
            Vec2::new(far.x - 1.0, mid.y),
            Vec2::new(1.0, 0.0),
            Vec2::new(far.x, mid.y),
        ),
        (
            Vec2::new(mid.x, 1.0),
            Vec2::new(0.0, -1.0),
            Vec2::new(mid.x, 0.0),
        ),
        (
            Vec2::new(mid.x, far.y - 1.0),
            Vec2::new(0.0, 1.0),
            Vec2::new(mid.x, far.y),
        ),
    ]
}

#[test]
fn the_player_stops_at_every_field_edge() {
    for (start, held, want) in field_edges(&Sim::new(SEED)) {
        let (mut sim, _) = world_of([player(start)]);

        for _ in 0..TICK_HZ {
            sim.tick(Input::new(held), &[]);
        }

        let landed = only_entity(&sim.snapshot()).pos;
        assert_eq!(landed, want, "holding {held} from {start} left the field");
    }
}

/// Only the blocked axis stops, so an edge is something to slide along rather
/// than stick to. The residual direction is the honest one: holding A at
/// `x = 0` really is travelling south-west.
#[test]
fn holding_into_an_edge_diagonally_slides_the_player_along_it() {
    let (mut sim, _) = world_of([player(Vec2::new(0.0, 8.0))]);

    for _ in 0..TICK_HZ {
        sim.tick(Input::new(Vec2::new(-0.71, 0.71)), &[]);
    }

    let view = only_entity(&sim.snapshot());
    assert_eq!(view.pos.x, 0.0, "he left the field: {}", view.pos);
    assert!(
        view.pos.y > 8.0,
        "he stuck instead of sliding: {}",
        view.pos
    );
    assert_eq!(view.facing, Facing::SouthWest);
}

/// Each screen cardinal drives straight at a field corner, where both tile
/// axes clamp at once and a held key produces exactly zero motion. Facing has
/// nothing to read, so it holds; locomotion still reports the intent, which is
/// the whole reason the snapshot publishes it.
#[test]
fn holding_into_a_corner_still_looks_like_running() {
    let (mut sim, _) = world_of([player(Vec2::ZERO)]);

    for _ in 0..TICK_HZ {
        sim.tick(Input::new(Vec2::new(-0.71, -0.71)), &[]);
    }

    let view = only_entity(&sim.snapshot());
    assert_eq!(view.pos, Vec2::ZERO);
    assert_eq!(view.facing, Facing::South, "facing changed with no motion");
    assert_eq!(view.locomotion, Locomotion::Running);
}

#[test]
fn a_new_world_holds_the_survivor_at_the_middle_of_the_field() {
    let sim = Sim::new(SEED);
    let middle = Vec2::new(
        (sim.terrain().width() / 2) as f32,
        (sim.terrain().height() / 2) as f32,
    );

    let snapshot = sim.snapshot();
    let view = only_entity(&snapshot);
    assert_eq!(view.pos, middle);
    assert_eq!(view.locomotion, Locomotion::Idle);
    assert_eq!(snapshot.player, Some(view.id));
}

#[test]
fn the_same_seed_and_input_sequence_replay_identically() {
    let run = || {
        let mut sim = Sim::new(SEED);
        for tick in 0..TICK_HZ as usize {
            let (_, held, _) = KEY_COMBINATIONS[tick % KEY_COMBINATIONS.len()];
            sim.tick(Input::new(held), &[]);
        }
        sim.snapshot()
    };

    assert_eq!(run(), run());
}
