use std::thread;
use std::time::{Duration, Instant};

use game::{Sim, TICK_DT};
use host::{alpha_for, spawn};

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
