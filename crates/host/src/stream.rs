//! Chunk streaming: deciding which chunks the world needs, generating them off
//! the simulation thread, and delivering each one to both consumers.
//!
//! Generation runs on a pool because cost grows with content. A chunk today is a
//! position hash and some noise; once it carries structures, carving and stamped
//! prefabs it is an order of magnitude more work, and a cold start is fifty of
//! them at once. That is a visible freeze on a thread that must not miss a tick.
//!
//! It is safe here for one specific reason: generation is a pure function of
//! position, so the order chunks complete in cannot change what they contain.
//! `worldgen` has a test that generates a block of chunks shuffled and compares
//! against sequential, which is the property this relies on.
//!
//! Both `game` and `render` need every chunk, so a finished chunk is shared by
//! `Arc` rather than moved. A `Box` would reach one of them, and generating
//! twice would be the same work done twice.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crossbeam_channel::{Receiver, Sender};
use game::Sim;
use worldgen::{ChunkCoord, ChunkView, World};

/// What the frontend is told about the resident window.
///
/// Reliable, and every message matters: a dropped `Ready` leaves a permanent
/// hole in the painted world, and a dropped `Dropped` leaks a node.
pub enum ChunkMessage {
    Ready(Arc<ChunkView>),
    Dropped(ChunkCoord),
}

/// A chunk to generate. The `issue` is what makes a stale result discardable.
struct Request {
    coord: ChunkCoord,
    issue: u64,
}

/// A generated chunk, tagged with the request it answers.
struct Done {
    coord: ChunkCoord,
    issue: u64,
    view: Arc<ChunkView>,
}

/// Owns residency, the pool, and the queues between them.
pub struct Streamer {
    /// Chunks within this many chunks of the centre, on each axis, are wanted.
    radius: i32,
    centre: ChunkCoord,
    /// Wanted and already delivered.
    loaded: BTreeSet<ChunkCoord>,
    /// Wanted and asked for, with the issue number of the newest request. A
    /// completion whose issue does not match this has been superseded.
    in_flight: BTreeMap<ChunkCoord, u64>,
    /// Increments per request, so a chunk that leaves and re-enters residency
    /// cannot be delivered twice.
    next_issue: u64,
    requests: Option<Sender<Request>>,
    completions: Receiver<Done>,
    workers: Vec<JoinHandle<()>>,
    out: Sender<ChunkMessage>,
}

impl Streamer {
    /// Spawns the pool and asks for the first window.
    pub fn new(
        world: Arc<World>,
        radius: u8,
        centre: ChunkCoord,
        out: Sender<ChunkMessage>,
    ) -> Self {
        let (requests, request_rx) = crossbeam_channel::unbounded::<Request>();
        let (done_tx, completions) = crossbeam_channel::unbounded::<Done>();

        let workers = (0..worker_count())
            .map(|n| {
                let world = Arc::clone(&world);
                let requests = request_rx.clone();
                let done = done_tx.clone();
                thread::Builder::new()
                    .name(format!("marrowfall-worldgen-{n}"))
                    .spawn(move || {
                        // Ends when the request sender is dropped, which is the
                        // shutdown signal.
                        for Request { coord, issue } in requests {
                            let view = Arc::new(worldgen::generate_chunk(&world, coord));
                            // A closed completion channel means the streamer is
                            // gone, so there is nothing left to do.
                            if done.send(Done { coord, issue, view }).is_err() {
                                return;
                            }
                        }
                    })
                    .expect("failed to spawn a worldgen worker")
            })
            .collect();

        let mut streamer = Self {
            radius: i32::from(radius),
            centre,
            loaded: BTreeSet::new(),
            in_flight: BTreeMap::new(),
            next_issue: 0,
            requests: Some(requests),
            completions,
            workers,
            out,
        };
        streamer.request_window();
        streamer
    }

    /// Moves the resident window if the centre has changed, then delivers
    /// whatever the pool has finished.
    ///
    /// Called once per tick from the simulation thread, which is where the
    /// player position already lives. Deciding residency anywhere else would let
    /// the simulation's own passability disagree with what the frontend paints.
    pub fn update(&mut self, centre: ChunkCoord, sim: &mut Sim) {
        if centre != self.centre {
            self.centre = centre;
            self.evict_outside_window(sim);
            self.request_window();
        }
        self.deliver(sim);
    }

    /// Whether every wanted chunk has arrived. The cold start is over when this
    /// is true.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        self.in_flight.is_empty()
    }

    #[must_use]
    pub fn loaded(&self) -> usize {
        self.loaded.len()
    }

    /// Asks for every wanted chunk that is neither loaded nor already asked for.
    fn request_window(&mut self) {
        let Some(requests) = self.requests.as_ref() else {
            return;
        };
        // Collected first: `window` borrows `self`, and the loop below mutates
        // the issue counter.
        let wanted: Vec<ChunkCoord> = self.window().collect();
        for coord in wanted {
            if self.loaded.contains(&coord) || self.in_flight.contains_key(&coord) {
                continue;
            }
            let issue = self.next_issue;
            self.next_issue += 1;
            self.in_flight.insert(coord, issue);
            // A closed channel means every worker is gone, which only happens
            // during teardown.
            let _ = requests.send(Request { coord, issue });
        }
    }

    /// Drops everything the window no longer covers, in both consumers.
    fn evict_outside_window(&mut self, sim: &mut Sim) {
        let wanted: BTreeSet<ChunkCoord> = self.window().collect();
        let gone: Vec<ChunkCoord> = self
            .loaded
            .iter()
            .copied()
            .filter(|coord| !wanted.contains(coord))
            .collect();
        for coord in gone {
            self.loaded.remove(&coord);
            sim.drop_chunk(coord);
            let _ = self.out.send(ChunkMessage::Dropped(coord));
        }
        // A request for a chunk that has since left is abandoned rather than
        // cancelled: the worker may already be building it, and the completion
        // will be discarded by the issue check in `deliver`.
        self.in_flight.retain(|coord, _| wanted.contains(coord));
    }

    /// Hands finished chunks to the simulation and the frontend.
    fn deliver(&mut self, sim: &mut Sim) {
        // `try_iter` so a tick never blocks on generation.
        for Done { coord, issue, view } in self.completions.try_iter().collect::<Vec<_>>() {
            // Superseded or evicted while in flight. Without this check the
            // frontend paints a chunk outside residency, and a coordinate that
            // left and re-entered arrives twice with colliding node names.
            if self.in_flight.get(&coord) != Some(&issue) {
                continue;
            }
            self.in_flight.remove(&coord);
            self.loaded.insert(coord);
            sim.insert_chunk(Arc::clone(&view));
            let _ = self.out.send(ChunkMessage::Ready(view));
        }
    }

    /// Every chunk the window covers, in a fixed order.
    fn window(&self) -> impl Iterator<Item = ChunkCoord> + '_ {
        let (r, centre) = (self.radius, self.centre);
        (-r..=r).flat_map(move |dy| {
            (-r..=r).map(move |dx| ChunkCoord::new(centre.x + dx, centre.y + dy))
        })
    }
}

impl Drop for Streamer {
    fn drop(&mut self) {
        // Closing the request channel is what ends the worker loops.
        self.requests = None;
        for worker in self.workers.drain(..) {
            // A panicked worker is not re-raised here: unwinding out of a drop
            // while already unwinding would abort the process.
            let _ = worker.join();
        }
    }
}

/// One worker per core, leaving one for the simulation thread itself.
fn worker_count() -> usize {
    thread::available_parallelism()
        .map(|n| n.get().saturating_sub(1).max(1))
        .unwrap_or(1)
}
