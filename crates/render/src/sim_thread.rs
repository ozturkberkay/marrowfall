//! The dedicated simulation thread and the two transports crossing it:
//! a command channel in (every message must arrive) and a latest-wins
//! triple buffer out (the renderer only ever wants the newest snapshot, and
//! superseded ones are dropped instead of queueing).
//!
//! Nothing in this module may touch Godot.

use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use game::{RenderSnapshot, Sim, TICK_DT};
use triple_buffer::{Input, Output, TripleBuffer};

/// Control messages into the simulation thread.
pub enum SimCommand {
    /// Stop ticking and exit the thread.
    Shutdown,
}

/// Main-thread handle to the running simulation.
pub struct SimHandle {
    commands: Sender<SimCommand>,
    snapshots: Output<RenderSnapshot>,
    thread: Option<JoinHandle<()>>,
}

impl SimHandle {
    /// The most recent snapshot the simulation has published.
    pub fn latest(&mut self) -> &RenderSnapshot {
        self.snapshots.read()
    }

    /// Asks the simulation thread to exit and blocks until it has.
    pub fn shutdown(mut self) {
        let _ = self.commands.send(SimCommand::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Spawns the simulation thread, moving `sim` onto it.
pub fn spawn(sim: Sim) -> SimHandle {
    let (commands, command_rx) = crossbeam_channel::unbounded();
    let (snapshot_tx, snapshots) = TripleBuffer::new(&sim.snapshot()).split();

    let thread = thread::Builder::new()
        .name("marrowfall-sim".into())
        .spawn(move || run(sim, &command_rx, snapshot_tx))
        .expect("failed to spawn simulation thread");

    SimHandle {
        commands,
        snapshots,
        thread: Some(thread),
    }
}

/// Self-paced fixed-timestep loop: sleep to the next deadline, run every due
/// tick, publish a snapshot each. Catch-up is capped so a stall slows the sim
/// rather than bursting.
fn run(mut sim: Sim, commands: &Receiver<SimCommand>, mut snapshots: Input<RenderSnapshot>) {
    const MAX_CATCH_UP_TICKS: u32 = 5;
    let tick_duration = Duration::from_secs_f64(TICK_DT);
    let mut next_tick = Instant::now() + tick_duration;

    loop {
        // Becomes a drain loop once commands beyond Shutdown exist.
        match commands.try_recv() {
            Ok(SimCommand::Shutdown) | Err(TryRecvError::Disconnected) => return,
            Err(TryRecvError::Empty) => {}
        }

        let now = Instant::now();
        if now < next_tick {
            thread::sleep(next_tick - now);
        }

        let mut ran = 0;
        while Instant::now() >= next_tick {
            sim.tick(&[]);
            snapshots.write(sim.snapshot());
            next_tick += tick_duration;
            ran += 1;
            if ran >= MAX_CATCH_UP_TICKS {
                // Too far behind: drop the lost time rather than burst-tick.
                next_tick = Instant::now() + tick_duration;
                break;
            }
        }
    }
}
