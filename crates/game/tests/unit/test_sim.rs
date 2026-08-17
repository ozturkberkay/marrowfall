use game::{Sim, TICK_HZ};

const SEED: u64 = 0x4D61_7272_6F77; // "Marrow"

#[test]
fn same_seed_generates_identical_terrain() {
    assert_eq!(Sim::new(SEED).terrain(), Sim::new(SEED).terrain());
}

#[test]
fn different_seeds_generate_different_terrain() {
    assert_ne!(Sim::new(SEED).terrain(), Sim::new(SEED + 1).terrain());
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
fn snapshot_of_empty_world_has_no_entities() {
    let mut sim = Sim::new(SEED);
    sim.tick(&[]);
    let snapshot = sim.snapshot();
    assert_eq!(snapshot.tick, 1);
    assert!(snapshot.entities.is_empty());
}
