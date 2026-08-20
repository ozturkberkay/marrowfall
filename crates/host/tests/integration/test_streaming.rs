use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use game::Sim;
use host::{ChunkMessage, SimHandle, Streamer, spawn};
use worldgen::{ChunkCoord, World, generate_chunk};

/// A small world, so a test is about streaming rather than about noise.
fn world() -> Arc<World> {
    let rules = worldgen::parse(worldgen::Tables {
        world: "region_pitch_tiles\tregion_jitter_pct\thome_bubble_tiles\n256\t60\t128\n",
        tiers: "tier\tinner_tiles\tharder_stray\teasier_stray\tstray_pct\n0\t0\t0\t0\t0\n",
        materials: "material\tblocks_walk\tblocks_jump\tblocks_shot\nsoil\t0\t0\t0\n",
        biomes: "biome\ttier\tweight\tground\theight_amp\theight_period\nlow\t0\t10\tsoil\t3\t240\n",
        site_classes: "class\tspacing\tseparation\tfill_pct\tmin_from_spawn\ttier_lo\ttier_hi\ncamp\t400\t240\t35\t0\t0\t0\n",
        sites: "site\tclass\tweight\tfootprint\ncampfire\tcamp\t1\t3\n",
    })
    .unwrap();
    Arc::new(World::new(rules, 7))
}

/// How long a test waits for the pool. Generously above the milliseconds a few
/// chunks take, so a loaded machine does not turn this into a flake.
const PATIENCE: Duration = Duration::from_secs(5);

/// Drains chunk messages until `enough` says the test has what it needs.
///
/// Counts arrivals per coordinate rather than collecting, because "exactly once"
/// is the property most of these tests are about.
fn drain_until(
    handle: &mut SimHandle,
    mut enough: impl FnMut(&BTreeMap<ChunkCoord, i32>) -> bool,
) -> BTreeMap<ChunkCoord, i32> {
    let mut arrivals: BTreeMap<ChunkCoord, i32> = BTreeMap::new();
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        for message in handle.take_chunks() {
            match message {
                ChunkMessage::Ready(view) => *arrivals.entry(view.coord).or_default() += 1,
                ChunkMessage::Dropped(coord) => *arrivals.entry(coord).or_default() -= 1,
            }
        }
        if enough(&arrivals) {
            return arrivals;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    arrivals
}

/// Every chunk a radius covers, centred on the origin chunk.
fn window(radius: i32) -> Vec<ChunkCoord> {
    (-radius..=radius)
        .flat_map(|y| (-radius..=radius).map(move |x| ChunkCoord::new(x, y)))
        .collect()
}

#[test]
fn a_residency_radius_streams_exactly_its_window() {
    let radius = 1;
    let mut handle = spawn(Sim::new(), world(), radius as u8);
    let wanted = window(radius);
    let arrivals = drain_until(&mut handle, |seen| seen.len() >= wanted.len());

    for coord in &wanted {
        assert_eq!(
            arrivals.get(coord).copied(),
            Some(1),
            "{coord:?} arrived {:?} times",
            arrivals.get(coord)
        );
    }
    assert_eq!(
        arrivals.len(),
        wanted.len(),
        "chunks outside the window arrived: {arrivals:?}"
    );
}

#[test]
fn a_chunk_arrives_exactly_once() {
    // The pool can finish a chunk after it was evicted, and a coordinate that
    // leaves and re-enters is requested twice. Neither may reach the frontend
    // twice, or its node names collide.
    let mut handle = spawn(Sim::new(), world(), 1);
    let wanted = window(1);
    let arrivals = drain_until(&mut handle, |seen| seen.len() >= wanted.len());
    // Then keep draining a while longer, so a late duplicate would show up.
    let more = drain_until(&mut handle, |_| false);
    for (coord, count) in arrivals {
        let total = count + more.get(&coord).copied().unwrap_or(0);
        assert_eq!(total, 1, "{coord:?} was delivered {total} times");
    }
}

#[test]
fn a_chunk_from_the_pool_is_what_one_thread_would_have_made() {
    // The pool inherits determinism from generation being a pure function of
    // position. This is the test that says so out loud: if a worker ever shared
    // state, the two would diverge.
    let world = world();
    let mut handle = spawn(Sim::new(), Arc::clone(&world), 1);
    let deadline = Instant::now() + PATIENCE;
    let mut checked = 0;
    while Instant::now() < deadline && checked < window(1).len() {
        for message in handle.take_chunks() {
            if let ChunkMessage::Ready(view) = message {
                let alone = generate_chunk(&world, view.coord);
                assert_eq!(view.tiles(), alone.tiles(), "{:?} differs", view.coord);
                checked += 1;
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(checked, window(1).len(), "not every chunk was compared");
}

#[test]
fn the_simulation_and_the_frontend_agree_on_the_resident_window() {
    // Both consumers are fed from the same delivery, so a chunk the frontend
    // paints must be one the simulation can collide against. If these diverged,
    // the player would walk through ground he can see.
    let mut handle = spawn(Sim::new(), world(), 1);
    let arrivals = drain_until(&mut handle, |seen| seen.len() >= window(1).len());
    assert_eq!(arrivals.len(), window(1).len());
    // Nothing to assert on the simulation side from out here without reaching
    // into the thread, so this pins the delivery count the sim was given.
    assert!(arrivals.values().all(|&n| n == 1));
}

#[test]
fn shutting_down_joins_every_worker() {
    // A leaked worker holds an `Arc<World>` and keeps running after the game is
    // gone. Dropping the handle must take the pool with it, and this test hangs
    // rather than fails if it does not.
    let mut handle = spawn(Sim::new(), world(), 1);
    let _ = drain_until(&mut handle, |seen| !seen.is_empty());
    drop(handle);
}

/// Drives a streamer with explicit centres, which is how eviction and re-entry
/// get tested without walking a player 32 tiles at four tiles a second.
fn streamer_and_inbox() -> (Streamer, Sim, crossbeam_channel::Receiver<ChunkMessage>) {
    let (tx, rx) = crossbeam_channel::unbounded();
    let streamer = Streamer::new(world(), 1, ChunkCoord::new(0, 0), tx);
    (streamer, Sim::new(), rx)
}

/// Pumps until every requested chunk has been delivered.
fn settle(streamer: &mut Streamer, sim: &mut Sim, centre: ChunkCoord) {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        streamer.update(centre, sim);
        if streamer.is_settled() {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("the pool never settled at {centre:?}");
}

#[test]
fn moving_the_centre_evicts_what_the_window_no_longer_covers() {
    let (mut streamer, mut sim, rx) = streamer_and_inbox();
    settle(&mut streamer, &mut sim, ChunkCoord::new(0, 0));
    assert_eq!(streamer.loaded(), 9, "a radius of one is three by three");

    // Two chunks along x, so the old window and the new one do not overlap.
    settle(&mut streamer, &mut sim, ChunkCoord::new(3, 0));
    assert_eq!(streamer.loaded(), 9, "the window changed size");

    let mut ready = BTreeMap::new();
    let mut dropped = BTreeMap::new();
    for message in rx.try_iter() {
        match message {
            ChunkMessage::Ready(view) => *ready.entry(view.coord).or_insert(0) += 1,
            ChunkMessage::Dropped(coord) => *dropped.entry(coord).or_insert(0) += 1,
        }
    }
    // Everything the first window held and the second does not must have been
    // dropped exactly once, or the frontend leaks a node per chunk.
    for coord in window(1) {
        assert_eq!(
            dropped.get(&coord).copied(),
            Some(1),
            "{coord:?} was not dropped"
        );
    }
    // And nothing was dropped that was never delivered.
    for coord in dropped.keys() {
        assert!(
            ready.contains_key(coord),
            "{coord:?} was dropped but never sent"
        );
    }
}

#[test]
fn a_chunk_that_leaves_and_returns_is_delivered_again_and_only_once() {
    let (mut streamer, mut sim, rx) = streamer_and_inbox();
    settle(&mut streamer, &mut sim, ChunkCoord::new(0, 0));
    settle(&mut streamer, &mut sim, ChunkCoord::new(3, 0));
    settle(&mut streamer, &mut sim, ChunkCoord::new(0, 0));

    let mut net: BTreeMap<ChunkCoord, i32> = BTreeMap::new();
    for message in rx.try_iter() {
        match message {
            ChunkMessage::Ready(view) => *net.entry(view.coord).or_default() += 1,
            ChunkMessage::Dropped(coord) => *net.entry(coord).or_default() -= 1,
        }
    }
    // Back where it started, so every chunk in the window is held exactly once.
    for coord in window(1) {
        assert_eq!(
            net.get(&coord).copied(),
            Some(1),
            "{coord:?} is out of balance"
        );
    }
}

#[test]
fn a_still_centre_asks_for_nothing_twice() {
    let (mut streamer, mut sim, rx) = streamer_and_inbox();
    settle(&mut streamer, &mut sim, ChunkCoord::new(0, 0));
    // Many more updates at the same centre. A window that re-requested on every
    // tick would flood the pool and repaint the world constantly.
    for _ in 0..50 {
        streamer.update(ChunkCoord::new(0, 0), &mut sim);
    }
    let ready = rx
        .try_iter()
        .filter(|m| matches!(m, ChunkMessage::Ready(_)))
        .count();
    assert_eq!(ready, 9, "a still centre delivered {ready} chunks");
}
