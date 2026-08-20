use crate::chunks::Chunks;
use crate::components::{Facing, Player, Position, Velocity};
use crate::snapshot::{EntityView, Locomotion, RenderSnapshot};
use crate::{Input, Intent, WorldVec};

/// Simulation ticks per second. Frontends must call [`Sim::tick`] at this
/// rate (via a fixed-timestep accumulator) for real-time play.
pub const TICK_HZ: u32 = 60;

/// Fixed duration of one simulation tick, in seconds.
pub const TICK_DT: f64 = 1.0 / TICK_HZ as f64;

/// How fast held input walks the player, in tile units per second.
///
/// A playtest starting point, not a derived figure. The bake's camera elevation
/// does not match the tile projection, so sprite height is foreshortened and
/// ground travel is not. No arithmetic turns one into the other, so tune this
/// by eye against the run cycle's foot slide.
pub const PLAYER_SPEED: f64 = 4.0;

/// What to create an entity with. Named fields, so `at` and `velocity` cannot
/// be swapped at a call site, and later components are additive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spawn {
    pub at: WorldVec,
    /// Tile units per second, or `None` for something that never moves, which
    /// integration then skips entirely.
    pub velocity: Option<WorldVec>,
    /// Whether held input drives this entity. Only one is `true` today. A
    /// second one takes the same input as the first.
    pub player: bool,
}

/// The authoritative game world.
pub struct Sim {
    world: hecs::World,
    /// The streamed window of the world. Filled from outside, because
    /// generation is `host`'s job and this crate does no threading.
    chunks: Chunks,
    ticks: u64,
}

impl Default for Sim {
    fn default() -> Self {
        Self::new()
    }
}

impl Sim {
    /// The survivor, at the world origin.
    ///
    /// The origin and not a field centre: every difficulty band and the home
    /// bubble measure from there, so spawning anywhere else would put the player
    /// at an arbitrary distance into the world.
    #[must_use]
    pub fn new() -> Self {
        Self::with_entities(&[Spawn {
            at: WorldVec::ZERO,
            velocity: Some(WorldVec::ZERO),
            player: true,
        }])
        .0
    }

    /// A world with a starting cast, and their [`EntityView::id`]s in the
    /// order given.
    ///
    /// Entities enter here or from inside a tick, never from outside one, so
    /// the world stays a function of how it was built plus the intent stream.
    /// This stands in for worldgen until that lands.
    ///
    /// # Panics
    /// If any position or velocity is not finite. A non-finite position
    /// spreads through every later tick and makes a snapshot unequal to
    /// itself, which would break any replay comparison. Also if a player spawn
    /// carries no velocity, because nothing can then move it.
    #[must_use]
    pub fn with_entities(entities: &[Spawn]) -> (Self, Vec<u64>) {
        let mut sim = Self {
            world: hecs::World::new(),
            chunks: Chunks::default(),
            ticks: 0,
        };
        let ids = entities.iter().map(|&spawn| sim.spawn(spawn)).collect();
        (sim, ids)
    }

    /// The ground the simulation currently knows about.
    #[must_use]
    pub fn chunks(&self) -> &Chunks {
        &self.chunks
    }

    /// Takes a generated chunk. Called by whoever owns generation, once per
    /// chunk, before the tick that needs it.
    pub fn insert_chunk(&mut self, view: std::sync::Arc<worldgen::ChunkView>) {
        self.chunks.insert(view);
    }

    /// Drops a chunk that has left the resident window.
    pub fn drop_chunk(&mut self, coord: worldgen::ChunkCoord) {
        self.chunks.remove(coord);
    }

    /// Which chunk the player is standing in, which is what residency follows.
    #[must_use]
    pub fn player_chunk(&self) -> Option<worldgen::ChunkCoord> {
        self.world
            .query::<hecs::With<&Position, &Player>>()
            .iter()
            .next()
            .map(|position| {
                worldgen::ChunkCoord::of(worldgen::IVec2::new(
                    position.current.x.floor() as i32,
                    position.current.y.floor() as i32,
                ))
            })
    }

    /// Number of completed ticks.
    #[must_use]
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Elapsed simulation time in seconds.
    #[must_use]
    pub fn time(&self) -> f64 {
        self.ticks as f64 * TICK_DT
    }

    /// Creates an entity and returns its [`EntityView::id`]. Private on
    /// purpose: see [`Sim::with_entities`].
    fn spawn(&mut self, spawn: Spawn) -> u64 {
        assert!(spawn.at.is_finite(), "spawn position must be finite");
        assert!(
            spawn.velocity.is_none_or(WorldVec::is_finite),
            "spawn velocity must be finite"
        );

        assert!(
            !spawn.player || spawn.velocity.is_some(),
            "a player spawn must carry a velocity, or input could not move it"
        );

        let mut entity = hecs::EntityBuilder::new();
        entity.add(Position::new(spawn.at)).add(Facing::South);
        if let Some(velocity) = spawn.velocity {
            entity.add(Velocity(velocity));
        }
        if spawn.player {
            entity.add(Player);
        }
        self.world.spawn(entity.build()).to_bits().get()
    }

    /// Advances the world by exactly one [`TICK_DT`] step.
    ///
    /// `input` is read once, so skipping ticks loses no held state.
    pub fn tick(&mut self, input: Input, intents: &[Intent]) {
        // `Intent` is uninhabited until the first gameplay milestone; there
        // is nothing to dispatch yet.
        let _ = intents;

        // Order is load-bearing. The carry must precede anything that moves,
        // and anything that reads `Position::previous`. Keep the two loops
        // separate: folding the carry into the integration query would skip
        // every entity without a `Velocity`. Nothing but integration writes
        // `current` yet, so no test catches that; the first teleport has to
        // arrive with one.
        self.carry_positions_forward();
        // Before integration, so a key pressed this tick lands this tick.
        self.apply_input(input);
        self.apply_velocity();
        // Reads the motion actually applied, so it has to follow everything
        // that can move an entity this tick, collision included.
        self.apply_facing();

        self.ticks += 1;
    }

    /// A render-ready copy of every positioned entity this tick.
    ///
    /// Order follows storage layout rather than spawn order, so it shifts as
    /// the world changes. Identical histories still produce identical order.
    #[must_use]
    pub fn snapshot(&self) -> RenderSnapshot {
        let entities = self
            .world
            .query::<(hecs::Entity, &Position, &Facing, Option<&Velocity>)>()
            .iter()
            .map(|(entity, position, facing, velocity)| EntityView {
                id: entity.to_bits().get(),
                pos: position.current,
                prev_pos: position.previous,
                facing: *facing,
                locomotion: locomotion_of(velocity),
                // Carried across the boundary because the frontend cannot work
                // it out: it may not hold the chunk the simulation used, and the
                // two must agree on where a character's feet are.
                height: self
                    .chunks
                    .tile(tile_of(position.current))
                    .map_or(0, |tile| tile.height),
            })
            .collect();

        RenderSnapshot {
            tick: self.ticks,
            time: self.time(),
            entities,
            player: self.player_id(),
        }
    }

    fn player_id(&self) -> Option<u64> {
        self.world
            .query::<hecs::With<hecs::Entity, &Player>>()
            .iter()
            .next()
            .map(|entity| entity.to_bits().get())
    }

    fn carry_positions_forward(&mut self) {
        for position in self.world.query_mut::<&mut Position>() {
            position.previous = position.current;
        }
    }

    /// Facing follows the motion just applied, not the velocity that asked
    /// for it: an entity walking into a wall has velocity but no motion.
    /// A teleport assigns `previous` to match, so it moves nothing here.
    fn apply_facing(&mut self) {
        for (position, facing) in self.world.query_mut::<(&Position, &mut Facing)>() {
            if let Some(direction) = Facing::from_direction(position.current - position.previous) {
                *facing = direction;
            }
        }
    }

    /// Held input sets the player's velocity outright, not a force. This is a
    /// game of exact positioning, so a key press must move him this tick.
    fn apply_input(&mut self, input: Input) {
        // The one place a direction becomes a world displacement, so the
        // widening happens here rather than being scattered.
        let velocity = input.move_dir().as_dvec2() * PLAYER_SPEED;
        for player in self.world.query_mut::<hecs::With<&mut Velocity, &Player>>() {
            player.0 = velocity;
        }
    }

    /// Integrates velocity, one axis at a time, refusing a step the ground does
    /// not allow.
    ///
    /// Per axis rather than as one move, and that is what produces the slide: an
    /// entity walking into a cliff at an angle keeps the component that is still
    /// open instead of stopping dead. It is also why nothing can slip through the
    /// corner where two cliffs meet, since a diagonal step is never asked for.
    ///
    /// `previous` is deliberately left alone. A shortened move must not carry it
    /// along, or the motion that facing reads disappears.
    fn apply_velocity(&mut self) {
        let chunks = &self.chunks;
        for (position, velocity) in self.world.query_mut::<(&mut Position, &Velocity)>() {
            let wanted = position.current + velocity.0 * TICK_DT;
            for axis in [Axis::X, Axis::Y] {
                let candidate = axis.blend(position.current, wanted);
                if chunks.can_step(tile_of(position.current), tile_of(candidate)) {
                    position.current = candidate;
                }
            }
        }
    }
}

/// Which coordinate a move is being tried along.
#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
}

impl Axis {
    /// `wanted` on this axis, `current` on the other.
    fn blend(self, current: WorldVec, wanted: WorldVec) -> WorldVec {
        match self {
            Self::X => WorldVec::new(wanted.x, current.y),
            Self::Y => WorldVec::new(current.x, wanted.y),
        }
    }
}

/// Which tile a world position stands on.
///
/// Floors, so a position anywhere inside a tile belongs to it and a negative
/// coordinate does not round toward zero into its neighbour.
fn tile_of(at: WorldVec) -> worldgen::IVec2 {
    worldgen::IVec2::new(at.x.floor() as i32, at.y.floor() as i32)
}

/// Running whenever a velocity asks for motion, even at a wall where nothing
/// moves. Derived here, so nothing holds a second copy of it.
fn locomotion_of(velocity: Option<&Velocity>) -> Locomotion {
    match velocity {
        Some(Velocity(asked)) if *asked != WorldVec::ZERO => Locomotion::Running,
        _ => Locomotion::Idle,
    }
}
