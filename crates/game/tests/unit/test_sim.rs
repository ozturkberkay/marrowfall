use game::{EntityView, Facing, RenderSnapshot, Sim, Spawn, TICK_HZ, Vec2};

const SEED: u64 = 0x4D61_7272_6F77; // "Marrow"

/// Deliberately awkward magnitudes on both axes, so no assertion can lean on
/// a velocity that happens to round cleanly.
const DRIFT: Vec2 = Vec2::new(3.0, -7.0);

/// One axis each. Named for the axis, not a screen direction: which way `+y`
/// points on screen is the frontend's business, not the simulation's.
const ALONG_X: Vec2 = Vec2::new(3.0, 0.0);
const ALONG_Y: Vec2 = Vec2::new(0.0, -7.0);

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
            },
        ]);
        for _ in 0..TICK_HZ {
            sim.tick(&[]);
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
        sim.tick(&[]);
    }
    assert_eq!(sim.ticks(), u64::from(TICK_HZ));
    assert!((sim.time() - 1.0).abs() < 1e-9);
}

#[test]
fn a_snapshot_reports_the_tick_and_time_it_describes() {
    let mut sim = Sim::new(SEED);
    for _ in 0..TICK_HZ {
        sim.tick(&[]);
    }

    let snapshot = sim.snapshot();
    assert_eq!(snapshot.tick, u64::from(TICK_HZ));
    assert!((snapshot.time - 1.0).abs() < 1e-9);
}

#[test]
fn snapshot_of_empty_world_has_no_entities() {
    let mut sim = Sim::new(SEED);
    sim.tick(&[]);
    assert!(sim.snapshot().entities.is_empty());
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

    sim.tick(&[]);

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
        sim.tick(&[]);
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
        sim.tick(&[]);
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

    sim.tick(&[]);
    let after_one = sim.snapshot();
    sim.tick(&[]);
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
        }]);

        for _ in 0..TICK_HZ {
            sim.tick(&[]);
        }

        let view = only_entity(&sim.snapshot());
        assert_eq!(view.pos, Vec2::new(4.0, 4.0), "{velocity:?} drifted");
        assert_eq!(view.prev_pos, view.pos);
    }
}

/// Tile directions and the screen direction each one points, measured against
/// Godot's isometric tilemap. Both tile axes run down the screen, so the screen
/// cardinals are tile diagonals.
const FACINGS: [(Vec2, Facing); 8] = [
    (Vec2::new(1.0, 1.0), Facing::South),
    (Vec2::new(1.0, 0.0), Facing::SouthEast),
    (Vec2::new(1.0, -1.0), Facing::East),
    (Vec2::new(0.0, -1.0), Facing::NorthEast),
    (Vec2::new(-1.0, -1.0), Facing::North),
    (Vec2::new(-1.0, 0.0), Facing::NorthWest),
    (Vec2::new(-1.0, 1.0), Facing::West),
    (Vec2::new(0.0, 1.0), Facing::SouthWest),
];

#[test]
fn a_new_entity_faces_south() {
    let (sim, _) = world_of([moving(Vec2::ZERO, DRIFT)]);
    assert_eq!(only_entity(&sim.snapshot()).facing, Facing::South);
}

#[test]
fn facing_follows_the_direction_travelled() {
    for (velocity, want) in FACINGS {
        let (mut sim, _) = world_of([moving(Vec2::ZERO, velocity)]);
        sim.tick(&[]);
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
        sim.tick(&[]);
        assert_eq!(
            only_entity(&sim.snapshot()).facing,
            want,
            "travelling {velocity} should look {want:?}"
        );
    }
}

/// The related case, an entity that moves and then stops keeping its facing,
/// cannot be tested yet: velocity is fixed at spawn, and a stopped entity is
/// indistinguishable from one that spawned still and already faced south. It
/// arrives with the first thing that can change velocity.
#[test]
fn facing_holds_steady_while_travelling() {
    let (mut sim, _) = world_of([moving(Vec2::ZERO, Vec2::new(-1.0, -1.0))]);

    for _ in 0..TICK_HZ {
        sim.tick(&[]);
        assert_eq!(only_entity(&sim.snapshot()).facing, Facing::North);
    }
}

#[test]
fn a_still_entity_keeps_facing_south() {
    for velocity in [None, Some(Vec2::ZERO)] {
        let (mut sim, _) = world_of([Spawn {
            at: Vec2::new(4.0, 4.0),
            velocity,
        }]);
        sim.tick(&[]);
        assert_eq!(only_entity(&sim.snapshot()).facing, Facing::South);
    }
}
