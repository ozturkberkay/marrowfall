use game::{
    EntityView, Facing, Input, Locomotion, PLAYER_SPEED, RenderSnapshot, Sim, Spawn, TICK_HZ, Vec2,
    WorldVec,
};

/// Deliberately awkward magnitudes on both axes, so no assertion can lean on
/// a velocity that happens to round cleanly.
const DRIFT: WorldVec = WorldVec::new(3.0, -7.0);

/// One axis each. Named for the axis, not a screen direction: which way `+y`
/// points on screen is the frontend's business, not the simulation's.
const ALONG_X: WorldVec = WorldVec::new(3.0, 0.0);
const ALONG_Y: WorldVec = WorldVec::new(0.0, -7.0);

/// Far enough inside the field that a second of walking cannot reach an edge.
/// So a test about speed is not also a test about the clamp.
const MIDFIELD: WorldVec = WorldVec::new(8.0, 8.0);

/// A world holding exactly these entities, with their ids positionally, so a
/// caller can destructure them by name.
fn world_of<const N: usize>(spawns: [Spawn; N]) -> (Sim, [u64; N]) {
    let (mut sim, ids) = Sim::with_entities(&spawns);
    give_it_ground(&mut sim);
    (sim, ids.try_into().expect("one id per spawn"))
}

/// Flat, open chunks around the origin.
///
/// Without them nothing moves, and that is correct rather than a nuisance: an
/// unloaded tile is not open ground, so a simulation that has been handed no
/// chunks has nowhere to walk. Every movement test therefore has to say what it
/// is standing on.
fn give_it_ground(sim: &mut Sim) {
    let rules = worldgen::parse(worldgen::Tables {
        world: "region_pitch_tiles\tregion_jitter_pct\thome_bubble_tiles\n256\t0\t4096\n",
        tiers: "tier\tinner_tiles\tharder_stray\teasier_stray\tstray_pct\n0\t0\t0\t0\t0\n",
        materials: "material\tblocks_walk\tblocks_jump\tblocks_shot\nsoil\t0\t0\t0\n",
        // Zero amplitude, so the ground is level and a movement assertion is
        // about movement rather than about a step it happened to meet.
        biomes: "biome\ttier\tweight\tground\theight_amp\theight_period\nlow\t0\t10\tsoil\t0\t240\n",
        site_classes: "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\ncamp\t400\t240\t35\t0\t0\t0\n",
        sites: "site\tclass\tweight\tfootprint\ncampfire\tcamp\t1\t3\n",
    })
    .unwrap();
    let world = worldgen::World::new(rules, 7);
    for y in -2..=2 {
        for x in -2..=2 {
            let coord = worldgen::ChunkCoord::new(x, y);
            sim.insert_chunk(std::sync::Arc::new(worldgen::generate_chunk(&world, coord)));
        }
    }
}

fn moving(at: WorldVec, velocity: WorldVec) -> Spawn {
    Spawn {
        at,
        velocity: Some(velocity),
        player: false,
    }
}

/// The one entity that held input drives. It starts still, because input is the
/// only thing that gives it a velocity.
fn player(at: WorldVec) -> Spawn {
    Spawn {
        at,
        velocity: Some(WorldVec::ZERO),
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
fn a_new_simulation_holds_no_ground_yet() {
    // Terrain streams in from outside, so a fresh simulation knows nothing about
    // the world until chunks arrive. Anything reading the ground has to treat
    // that as "not known", never as open space.
    let sim = Sim::new();
    assert!(sim.chunks().is_empty());
    assert_eq!(sim.chunks().tile(worldgen::IVec2::ZERO), None);
}

#[test]
fn the_same_world_built_the_same_way_replays_identically() {
    let run = || {
        let (mut sim, _) = world_of([
            moving(WorldVec::new(1.0, 2.0), DRIFT),
            Spawn {
                at: WorldVec::new(5.0, 5.0),
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
    let mut sim = Sim::new();
    for _ in 0..TICK_HZ {
        sim.tick(Input::default(), &[]);
    }
    assert_eq!(sim.ticks(), u64::from(TICK_HZ));
    assert!((sim.time() - 1.0).abs() < 1e-9);
}

#[test]
fn a_snapshot_reports_the_tick_and_time_it_describes() {
    let mut sim = Sim::new();
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
    let (sim, [id]) = world_of([moving(WorldVec::new(4.0, 5.0), DRIFT)]);

    let view = only_entity(&sim.snapshot());
    assert_eq!(view.id, id);
    assert_eq!(view.pos, WorldVec::new(4.0, 5.0));
}

#[test]
fn a_new_entity_has_nothing_to_interpolate() {
    let (sim, _) = world_of([moving(WorldVec::new(4.0, 5.0), DRIFT)]);

    let view = only_entity(&sim.snapshot());
    assert_eq!(view.prev_pos, view.pos);
}

#[test]
fn a_tick_leaves_both_ends_of_the_move_to_interpolate() {
    let start = WorldVec::new(1.0, 1.0);
    let (mut sim, _) = world_of([moving(start, DRIFT)]);

    sim.tick(Input::default(), &[]);

    let view = only_entity(&sim.snapshot());
    assert_eq!(view.prev_pos, start);
    assert!(view.pos.x > start.x, "x never advanced: {}", view.pos);
    assert!(view.pos.y < start.y, "y never advanced: {}", view.pos);
}

#[test]
fn a_second_of_ticks_moves_each_entity_by_its_own_velocity() {
    let (mut sim, [along_x, along_y]) = world_of([
        moving(WorldVec::ZERO, ALONG_X),
        moving(WorldVec::ZERO, ALONG_Y),
    ]);

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
    let (mut sim, _) = world_of([moving(WorldVec::ZERO, DRIFT)]);

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
        moving(WorldVec::ZERO, ALONG_X),
        moving(WorldVec::new(9.0, 9.0), ALONG_Y),
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
    for velocity in [None, Some(WorldVec::ZERO)] {
        let (mut sim, _) = world_of([Spawn {
            at: WorldVec::new(4.0, 4.0),
            velocity,
            player: false,
        }]);

        for _ in 0..TICK_HZ {
            sim.tick(Input::default(), &[]);
        }

        let view = only_entity(&sim.snapshot());
        assert_eq!(view.pos, WorldVec::new(4.0, 4.0), "{velocity:?} drifted");
        assert_eq!(view.prev_pos, view.pos);
    }
}

/// Tile directions and the screen direction each one points, measured against
/// Godot's isometric tilemap. Both tile axes run down the screen, so the screen
/// cardinals are tile diagonals.
const FACINGS: [(WorldVec, Facing, &str); 8] = [
    (WorldVec::new(1.0, 1.0), Facing::South, "s"),
    (WorldVec::new(1.0, 0.0), Facing::SouthEast, "se"),
    (WorldVec::new(1.0, -1.0), Facing::East, "e"),
    (WorldVec::new(0.0, -1.0), Facing::NorthEast, "ne"),
    (WorldVec::new(-1.0, -1.0), Facing::North, "n"),
    (WorldVec::new(-1.0, 0.0), Facing::NorthWest, "nw"),
    (WorldVec::new(-1.0, 1.0), Facing::West, "w"),
    (WorldVec::new(0.0, 1.0), Facing::SouthWest, "sw"),
];

#[test]
fn a_new_entity_faces_south() {
    let (sim, _) = world_of([moving(WorldVec::ZERO, DRIFT)]);
    assert_eq!(only_entity(&sim.snapshot()).facing, Facing::South);
}

#[test]
fn facing_follows_the_direction_travelled() {
    for (velocity, want, _) in FACINGS {
        let (mut sim, _) = world_of([moving(WorldVec::ZERO, velocity)]);
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
        (WorldVec::new(1.0, 0.4), Facing::SouthEast),
        (WorldVec::new(1.0, 0.5), Facing::South),
        (WorldVec::new(0.4, 1.0), Facing::SouthWest),
        (WorldVec::new(0.5, 1.0), Facing::South),
    ];
    for (velocity, want) in cases {
        let (mut sim, _) = world_of([moving(WorldVec::ZERO, velocity)]);
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
    let (mut sim, _) = world_of([moving(WorldVec::ZERO, WorldVec::new(-1.0, -1.0))]);

    for _ in 0..TICK_HZ {
        sim.tick(Input::default(), &[]);
        assert_eq!(only_entity(&sim.snapshot()).facing, Facing::North);
    }
}

#[test]
fn a_still_entity_keeps_facing_south() {
    for velocity in [None, Some(WorldVec::ZERO)] {
        let (mut sim, _) = world_of([Spawn {
            at: WorldVec::new(4.0, 4.0),
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
/// projection turns it into. Written as the exact integer ratio that projection
/// produces.
///
/// Magnitude does not matter twice over: `Input::new` scales it to unit length,
/// and facing quantises on the ratio. The two-key rows are the near boundary
/// cases, at exactly 1/3 against a sector edge of 0.414.
const KEY_COMBINATIONS: [(&str, Vec2, Facing); 8] = [
    ("W", Vec2::new(-1.0, -1.0), Facing::North),
    ("W+D", Vec2::new(-1.0, -3.0), Facing::NorthEast),
    ("D", Vec2::new(1.0, -1.0), Facing::East),
    ("S+D", Vec2::new(3.0, 1.0), Facing::SouthEast),
    ("S", Vec2::new(1.0, 1.0), Facing::South),
    ("S+A", Vec2::new(1.0, 3.0), Facing::SouthWest),
    ("A", Vec2::new(-1.0, 1.0), Facing::West),
    ("W+A", Vec2::new(-3.0, -1.0), Facing::NorthWest),
];

/// Held north, named rather than indexed at each use.
const HOLDING_W: Vec2 = KEY_COMBINATIONS[0].1;

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
    let want = MIDFIELD + WorldVec::new(PLAYER_SPEED, 0.0);
    assert!(
        landed.abs_diff_eq(want, 1e-3),
        "landed at {landed}, not {want}"
    );
}

/// The frontend hands over a unit direction, so a diagonal covers the same
/// ground as a cardinal, not 1.41 times as much.
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

    for _ in 0..TICK_HZ / 2 {
        sim.tick(Input::new(HOLDING_W), &[]);
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

#[test]
fn holding_a_direction_moves_him_and_reads_as_running() {
    // The interesting half of this, a velocity that asks for motion where none
    // happens, needs something to walk into. Terrain collision is what brings it
    // back, and it is where the "running at a wall" case belongs.
    let (mut sim, _) = world_of([player(WorldVec::ZERO)]);

    for _ in 0..TICK_HZ {
        sim.tick(Input::new(HOLDING_W), &[]);
    }

    let view = only_entity(&sim.snapshot());
    assert_ne!(view.pos, WorldVec::ZERO, "held input moved nothing");
    assert_eq!(view.facing, Facing::North);
    assert_eq!(view.locomotion, Locomotion::Running);
}

#[test]
fn the_survivor_spawns_at_the_world_origin() {
    // Not a field centre: every difficulty band and the home bubble measure from
    // the origin, so spawning elsewhere would drop the player at an arbitrary
    // distance into the world.
    let sim = Sim::new();
    let view = only_entity(&sim.snapshot());
    assert_eq!(view.pos, WorldVec::ZERO);
}

#[test]
fn the_same_seed_and_input_sequence_replay_identically() {
    let run = || {
        let mut sim = Sim::new();
        for tick in 0..TICK_HZ as usize {
            let (_, held, _) = KEY_COMBINATIONS[tick % KEY_COMBINATIONS.len()];
            sim.tick(Input::new(held), &[]);
        }
        sim.snapshot()
    };

    assert_eq!(run(), run());
}
