use glam::Vec2;

use crate::Intent;
use crate::components::{Facing, Position, Velocity};
use crate::snapshot::{EntityView, RenderSnapshot};
use crate::terrain::TerrainGrid;

/// Simulation ticks per second. Frontends must call [`Sim::tick`] at this
/// rate (via a fixed-timestep accumulator) for real-time play.
pub const TICK_HZ: u32 = 60;

/// Fixed duration of one simulation tick, in seconds.
pub const TICK_DT: f64 = 1.0 / TICK_HZ as f64;

/// One tick in the precision the spatial maths uses, so no system has to cast
/// [`TICK_DT`] itself. The simulation keeps an f64 clock over f32 space.
const TICK_DT_F32: f32 = 1.0 / TICK_HZ as f32;

/// Side length of the placeholder starting area, in tiles. Becomes
/// zone-driven once real worldgen lands.
const WORLD_SIZE: u32 = 24;

/// What to create an entity with. Named fields, so `at` and `velocity` cannot
/// be swapped at a call site, and later components are additive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spawn {
    pub at: Vec2,
    /// Tile units per second, or `None` for something that never moves, which
    /// integration then skips entirely.
    pub velocity: Option<Vec2>,
}

/// The authoritative game world.
pub struct Sim {
    world: hecs::World,
    terrain: TerrainGrid,
    ticks: u64,
}

impl Sim {
    /// A world holding nothing but terrain.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self::with_entities(seed, &[]).0
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
    /// itself, which would break any replay comparison.
    #[must_use]
    pub fn with_entities(seed: u64, entities: &[Spawn]) -> (Self, Vec<u64>) {
        let mut sim = Self {
            world: hecs::World::new(),
            terrain: TerrainGrid::generate(seed, WORLD_SIZE, WORLD_SIZE),
            ticks: 0,
        };
        let ids = entities.iter().map(|&spawn| sim.spawn(spawn)).collect();
        (sim, ids)
    }

    /// Static ground layout. Fetched once by the frontend at startup; terrain
    /// does not change per tick, so it is not part of [`Sim::snapshot`].
    #[must_use]
    pub fn terrain(&self) -> &TerrainGrid {
        &self.terrain
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
            spawn.velocity.is_none_or(Vec2::is_finite),
            "spawn velocity must be finite"
        );

        let position = Position::new(spawn.at);
        let entity = match spawn.velocity {
            Some(velocity) => self
                .world
                .spawn((position, Facing::South, Velocity(velocity))),
            None => self.world.spawn((position, Facing::South)),
        };
        entity.to_bits().get()
    }

    /// Advances the world by exactly one [`TICK_DT`] step.
    pub fn tick(&mut self, intents: &[Intent]) {
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
            .query::<(hecs::Entity, &Position, &Facing)>()
            .iter()
            .map(|(entity, position, facing)| EntityView {
                id: entity.to_bits().get(),
                pos: position.current,
                prev_pos: position.previous,
                facing: *facing,
            })
            .collect();

        RenderSnapshot {
            tick: self.ticks,
            time: self.time(),
            entities,
        }
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

    fn apply_velocity(&mut self) {
        for (position, velocity) in self.world.query_mut::<(&mut Position, &Velocity)>() {
            position.current += velocity.0 * TICK_DT_F32;
        }
    }
}
