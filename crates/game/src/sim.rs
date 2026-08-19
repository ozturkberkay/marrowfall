use glam::Vec2;

use crate::components::{Facing, Player, Position, Velocity};
use crate::snapshot::{EntityView, Locomotion, RenderSnapshot};
use crate::terrain::TerrainGrid;
use crate::{Input, Intent};

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

/// How fast held input walks the player, in tile units per second.
///
/// A playtest starting point, not a derived figure. The bake's camera elevation
/// does not match the tile projection, so sprite height is foreshortened and
/// ground travel is not. No arithmetic turns one into the other, so tune this
/// by eye against the run cycle's foot slide.
pub const PLAYER_SPEED: f32 = 4.0;

/// What to create an entity with. Named fields, so `at` and `velocity` cannot
/// be swapped at a call site, and later components are additive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spawn {
    pub at: Vec2,
    /// Tile units per second, or `None` for something that never moves, which
    /// integration then skips entirely.
    pub velocity: Option<Vec2>,
    /// Whether held input drives this entity. Only one is `true` today. A
    /// second one takes the same input as the first.
    pub player: bool,
}

/// The authoritative game world.
pub struct Sim {
    world: hecs::World,
    terrain: TerrainGrid,
    ticks: u64,
}

impl Sim {
    /// Terrain plus the survivor, at the middle of the field.
    ///
    /// A placeholder for worldgen and a new-game flow, until those land.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let middle = Vec2::new((WORLD_SIZE / 2) as f32, (WORLD_SIZE / 2) as f32);
        Self::with_entities(
            seed,
            &[Spawn {
                at: middle,
                velocity: Some(Vec2::ZERO),
                player: true,
            }],
        )
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
        // Shortens a move instead of jumping, so it must come before facing.
        self.keep_player_on_the_field();
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
        let velocity = input.move_dir() * PLAYER_SPEED;
        for player in self.world.query_mut::<hecs::With<&mut Velocity, &Player>>() {
            player.0 = velocity;
        }
    }

    /// Stops the player at the edge of the painted field, which the camera
    /// follows him to.
    ///
    /// A placeholder for collision. It shortens a move instead of blocking it,
    /// so only the blocked axis stops and he slides along an edge. It leaves
    /// `previous` alone on purpose, see [`Position`].
    ///
    /// The player only. Everything else can leave the field, and the tests rely
    /// on that.
    fn keep_player_on_the_field(&mut self) {
        let far = Vec2::new(
            (self.terrain.width() - 1) as f32,
            (self.terrain.height() - 1) as f32,
        );
        for position in self.world.query_mut::<hecs::With<&mut Position, &Player>>() {
            position.current = position.current.clamp(Vec2::ZERO, far);
        }
    }

    fn apply_velocity(&mut self) {
        for (position, velocity) in self.world.query_mut::<(&mut Position, &Velocity)>() {
            position.current += velocity.0 * TICK_DT_F32;
        }
    }
}

/// Running whenever a velocity asks for motion, even at a wall where nothing
/// moves. Derived here, so nothing holds a second copy of it.
fn locomotion_of(velocity: Option<&Velocity>) -> Locomotion {
    match velocity {
        Some(Velocity(asked)) if *asked != Vec2::ZERO => Locomotion::Running,
        _ => Locomotion::Idle,
    }
}
