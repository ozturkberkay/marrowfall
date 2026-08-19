use std::thread;
use std::time::{Duration, Instant};

use game::{EntityView, Input, RenderSnapshot, Sim, TICK_DT, Vec2};
use host::{SimHandle, alpha_for, spawn};

fn tick() -> Duration {
    Duration::from_secs_f64(TICK_DT)
}

#[test]
fn alpha_is_zero_at_the_tick_deadline() {
    assert!((alpha_for(Duration::ZERO) - 0.0).abs() < 1e-6);
}

#[test]
fn alpha_is_the_fraction_of_a_tick_elapsed() {
    assert!((alpha_for(tick() / 2) - 0.5).abs() < 1e-6);
    assert!((alpha_for(tick() / 4) - 0.25).abs() < 1e-6);
}

#[test]
fn alpha_reaches_one_at_the_next_deadline() {
    assert!((alpha_for(tick()) - 1.0).abs() < 1e-6);
}

/// A late simulation thread must hold the newest known position rather than
/// extrapolate past it.
#[test]
fn alpha_clamps_once_the_next_tick_is_overdue() {
    assert!((alpha_for(tick() * 5) - 1.0).abs() < 1e-6);
}

#[test]
fn the_sim_thread_ticks_and_reports_itself_alive() {
    let mut handle = spawn(Sim::new(1));

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut saw_partway_through_a_tick = false;
    loop {
        let frame = handle.read();
        assert!(
            (0.0..=1.0).contains(&frame.alpha),
            "alpha out of range: {}",
            frame.alpha
        );
        saw_partway_through_a_tick |= frame.alpha > 0.0;
        if frame.snapshot.tick > 0 && saw_partway_through_a_tick {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "no tick with a live alpha within 2s"
        );
        thread::sleep(Duration::from_millis(5));
    }

    assert!(
        handle.is_alive(),
        "thread published a tick then reported dead"
    );
}

/// `Drop` joins the thread, so a bug there hangs forever rather than failing.
/// This is a deadlock guard; that `Drop` runs at all is structural, since it is
/// the only teardown path there is.
#[test]
fn dropping_the_handle_does_not_hang() {
    let handle = spawn(Sim::new(1));

    let start = Instant::now();
    drop(handle);
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "shutdown took {:?}",
        start.elapsed()
    );
}

/// The player of the world `Sim::new` builds, which is the only entity in it.
fn player_of(snapshot: &RenderSnapshot) -> EntityView {
    let id = snapshot.player.expect("Sim::new spawns a player");
    *snapshot
        .entities
        .iter()
        .find(|entity| entity.id == id)
        .expect("the player is missing from the snapshot")
}

/// Polls to a deadline rather than sleeping a fixed time: the simulation paces
/// itself off the wall clock, so any fixed sleep is either flaky or slow.
fn poll_until(handle: &mut SimHandle, ready: impl Fn(&RenderSnapshot) -> bool, complaint: &str) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !ready(handle.read().snapshot) {
        assert!(Instant::now() < deadline, "{complaint}");
        thread::sleep(Duration::from_millis(5));
    }
}

/// Latest-wins is only right for held state if one write outlives the tick that
/// read it. Half a tile is eight ticks at `PLAYER_SPEED`, so passing it proves
/// the buffer is not consumed on read.
#[test]
fn one_input_written_once_keeps_the_player_walking() {
    let mut handle = spawn(Sim::new(1));
    let start = player_of(handle.read().snapshot).pos;

    handle.set_input(Input::new(Vec2::new(1.0, 0.0)));

    poll_until(
        &mut handle,
        |snapshot| player_of(snapshot).pos.x > start.x + 0.5,
        "one input moved him less than eight ticks' worth",
    );
}

#[test]
fn a_player_nobody_is_driving_stands_still() {
    let mut handle = spawn(Sim::new(1));
    let start = player_of(handle.read().snapshot).pos;

    poll_until(&mut handle, |snapshot| snapshot.tick >= 30, "no ticks ran");

    assert_eq!(player_of(handle.read().snapshot).pos, start);
}
