use crate::Intent;
use crate::components::Position;
use crate::snapshot::{EntityView, RenderSnapshot};
use crate::terrain::TerrainGrid;

/// Simulation ticks per second. Frontends must call [`Sim::tick`] at this
/// rate (via a fixed-timestep accumulator) for real-time play.
pub const TICK_HZ: u32 = 60;

/// Fixed duration of one simulation tick, in seconds.
pub const TICK_DT: f64 = 1.0 / TICK_HZ as f64;

/// Side length of the placeholder starting area, in tiles. Becomes
/// zone-driven once real worldgen lands.
const WORLD_SIZE: u32 = 24;

/// The authoritative game world.
pub struct Sim {
    world: hecs::World,
    terrain: TerrainGrid,
    ticks: u64,
}

impl Sim {
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            world: hecs::World::new(),
            terrain: TerrainGrid::generate(seed, WORLD_SIZE, WORLD_SIZE),
            ticks: 0,
        }
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

    /// Advances the world by exactly one [`TICK_DT`] step.
    pub fn tick(&mut self, intents: &[Intent]) {
        // `Intent` is uninhabited until the first gameplay milestone; there
        // is nothing to dispatch yet.
        let _ = intents;
        // Gameplay systems (movement, combat, AI, ...) run here as they land.
        self.ticks += 1;
    }

    /// A render-ready copy of everything visible this tick.
    #[must_use]
    pub fn snapshot(&self) -> RenderSnapshot {
        let entities = self
            .world
            .query::<(hecs::Entity, &Position)>()
            .iter()
            .map(|(entity, position)| EntityView {
                id: entity.to_bits().get(),
                pos: position.0,
            })
            .collect();

        RenderSnapshot {
            tick: self.ticks,
            time: self.time(),
            entities,
        }
    }
}
