//! Runs the Marrowfall simulation on its own thread, and owns the three
//! transports that cross that boundary:
//!
//! - A command channel in. Every message must arrive.
//! - A latest-wins triple buffer of held [`game::Input`] in. Only what is held
//!   right now matters, so superseded samples are dropped.
//! - A latest-wins triple buffer of snapshots out. A frontend only ever wants
//!   the newest one.
//!
//! Both directions are latest-wins because the clocks do not match, not for
//! symmetry. One reliable input message per frame delivers 2.4 per tick at
//! 144 fps and half of one at 30. That makes walking speed a function of the
//! display.
//!
//! This is a crate rather than a module of the frontend because the frontend
//! cannot be tested: instantiating a Godot node needs a running engine, so
//! anything living there is unreachable from the test harness. Keeping the
//! pacing loop out here is what lets the catch-up cap and the shutdown path be
//! exercised headlessly. It also has no engine dependency, though that part is
//! nearly free: `Gd<T>` is not `Send`, so the compiler would have stopped a
//! Godot handle crossing onto this thread wherever the code lived.

use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use game::{Input, RenderSnapshot, Sim, TICK_DT};
// `Input` is aliased, because `game::Input` is the held player input this crate
// carries. The buffer's write end is a different thing.
use triple_buffer::{Input as Writer, Output, TripleBuffer};

/// Control messages into the simulation thread.
enum SimCommand {
    /// Stop ticking and exit the thread.
    Shutdown,
}

/// A snapshot and the deadline of the tick it describes.
///
/// The deadline travels inside the buffer rather than beside it so one atomic
/// swap moves both: read separately, a publish landing between the two reads
/// would pair a snapshot with another tick's deadline.
///
/// It cannot be derived as `epoch + tick * TICK_DT`, which is the obvious
/// simplification. The catch-up cap in [`run`] resets the schedule against the
/// wall clock, so a derived deadline drifts away from real pacing for good.
#[derive(Clone)]
struct Published {
    snapshot: RenderSnapshot,
    due_at: Instant,
}

/// The snapshot to draw, and how far into its tick this frame lands.
#[derive(Debug)]
pub struct Frame<'a> {
    pub snapshot: &'a RenderSnapshot,
    pub alpha: f32,
}

/// How far through a tick a frame lands: `0` at the tick's own deadline, `1` at
/// the next one.
///
/// [`SimHandle::read`] applies this. It is public so the mapping is pinned by a
/// test rather than by inspection.
#[must_use]
pub fn alpha_for(since_due: Duration) -> f32 {
    (since_due.as_secs_f64() / TICK_DT).clamp(0.0, 1.0) as f32
}

/// Main-thread handle to the running simulation. Dropping it stops the thread
/// and waits for it, so the simulation's lifetime tracks this value and
/// nothing else.
pub struct SimHandle {
    commands: Sender<SimCommand>,
    inputs: Writer<Input>,
    snapshots: Output<Published>,
    thread: Option<JoinHandle<()>>,
}

impl SimHandle {
    /// Replaces the held input the next tick reads.
    ///
    /// Safe to call any number of times per frame, including zero. With zero
    /// calls the previous value stands and the player keeps walking. Call it
    /// before [`Self::read`], which borrows the whole handle.
    ///
    /// The OS does not always send the key release, so a frontend must stop
    /// sampling its keyboard when the window loses focus. It writes a still
    /// input instead.
    pub fn set_input(&mut self, input: Input) {
        self.inputs.write(input);
    }

    /// The newest tick, with the interpolation `alpha` for that same tick.
    ///
    /// The returned [`Frame`] keeps this handle borrowed, so nothing else on
    /// the owner can be touched while it lives. Copy out what you need.
    pub fn read(&mut self) -> Frame<'_> {
        let published = self.snapshots.read();
        let since_due = Instant::now().saturating_duration_since(published.due_at);
        Frame {
            snapshot: &published.snapshot,
            alpha: alpha_for(since_due),
        }
    }

    /// Whether the simulation thread is still running.
    ///
    /// A panicked thread stops publishing, and the triple buffer keeps handing
    /// back the last snapshot with no error, so the world silently freezes.
    /// Poll this to notice, and report it somewhere a person will see.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
    }
}

impl Drop for SimHandle {
    fn drop(&mut self) {
        let _ = self.commands.send(SimCommand::Shutdown);
        if let Some(thread) = self.thread.take() {
            // A panic is reported through `is_alive`, not re-raised: unwinding
            // out of a drop while already unwinding would abort the process.
            let _ = thread.join();
        }
    }
}

/// Spawns the simulation thread, moving `sim` onto it.
///
/// The returned handle is the only route to the simulation. Terrain and
/// anything else read off `Sim` directly has to be taken before this call.
pub fn spawn(sim: Sim) -> SimHandle {
    let (commands, command_rx) = crossbeam_channel::unbounded();

    // One clock read for the seed and the loop both, so alpha stays continuous
    // across the seed-to-first-tick handover. Two would differ by however long
    // spawning the thread took.
    let epoch = Instant::now();
    let seed = Published {
        snapshot: sim.snapshot(),
        due_at: epoch,
    };
    let (snapshot_tx, snapshots) = TripleBuffer::new(&seed).split();
    let (inputs, input_rx) = TripleBuffer::new(&Input::default()).split();

    let thread = thread::Builder::new()
        .name("marrowfall-sim".into())
        .spawn(move || run(sim, &command_rx, input_rx, snapshot_tx, epoch))
        .expect("failed to spawn simulation thread");

    SimHandle {
        commands,
        inputs,
        snapshots,
        thread: Some(thread),
    }
}

/// Self-paced fixed-timestep loop: wait until the next deadline, run every due
/// tick, publish a snapshot each. Catch-up is capped so a stall slows the sim
/// rather than bursting.
fn run(
    mut sim: Sim,
    commands: &Receiver<SimCommand>,
    mut inputs: Output<Input>,
    mut snapshots: Writer<Published>,
    epoch: Instant,
) {
    const MAX_CATCH_UP_TICKS: u32 = 5;
    let tick_duration = Duration::from_secs_f64(TICK_DT);
    let mut next_tick = epoch + tick_duration;

    loop {
        // Waiting for the deadline and listening for commands are the same
        // operation. Sleeping first would leave a command sitting unread for
        // most of a tick, which is a whole tick of input latency once intents
        // travel this channel.
        match commands.recv_deadline(next_tick) {
            Ok(SimCommand::Shutdown) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }

        let mut ran = 0;
        while Instant::now() >= next_tick {
            // Once per tick, not once per catch-up burst. A tick is the unit
            // held input is defined over.
            sim.tick(*inputs.read(), &[]);
            // The deadline this tick was due at, captured before it advances.
            // Stamping `now` instead would fold this tick's compute time into
            // alpha as jitter, which is what interpolation exists to remove.
            snapshots.write(Published {
                snapshot: sim.snapshot(),
                due_at: next_tick,
            });
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
