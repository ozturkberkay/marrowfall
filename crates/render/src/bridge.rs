//! `GameBridge` — the single node through which the simulation reaches the
//! scene tree. It boots the sim, paints the ground the sim generated, and
//! consumes one snapshot per rendered frame. All Godot access stays on the
//! main thread.

use godot::classes::{Camera2D, TileMapLayer};
use godot::prelude::*;

use game::{Sim, TerrainGrid};

use crate::sim_thread::{self, SimHandle};

/// Fixed development seed; replaced by the new-game/save flow later.
const DEV_SEED: u64 = 0x4D61_7272_6F77; // "Marrow"

#[derive(GodotClass)]
#[class(init, base=Node)]
pub struct GameBridge {
    base: Base<Node>,
    /// TileMapLayer the sim's ground terrain is painted into.
    #[export]
    #[init(val = NodePath::from("../Ground"))]
    ground_path: NodePath,
    /// Camera framed on the generated world at startup.
    #[export]
    #[init(val = NodePath::from("../Camera2D"))]
    camera_path: NodePath,
    sim: Option<SimHandle>,
    last_seen_tick: u64,
}

#[godot_api]
impl INode for GameBridge {
    fn ready(&mut self) {
        let sim = Sim::new(DEV_SEED);
        self.paint_ground(sim.terrain());
        self.frame_camera(sim.terrain());
        self.sim = Some(sim_thread::spawn(sim));
    }

    fn process(&mut self, _delta: f64) {
        let Some(sim) = self.sim.as_mut() else {
            return;
        };
        let snapshot = sim.latest();

        // Entity nodes will be spawned/updated from `snapshot` here once the
        // world has entities; for now just surface that the pipeline moves.
        if self.last_seen_tick == 0 && snapshot.tick > 0 {
            godot_print!(
                "[marrowfall] sim thread live: tick {} (t = {:.2}s)",
                snapshot.tick,
                snapshot.time
            );
        }
        self.last_seen_tick = snapshot.tick;
    }

    fn exit_tree(&mut self) {
        if let Some(sim) = self.sim.take() {
            sim.shutdown();
        }
    }
}

impl GameBridge {
    fn ground(&self) -> Gd<TileMapLayer> {
        self.base().get_node_as::<TileMapLayer>(&self.ground_path)
    }

    /// Paints the sim's ground grid into the TileMapLayer: variant `n` maps
    /// to atlas column `n` of source 0 (see `ground.tileset.tres`).
    fn paint_ground(&self, terrain: &TerrainGrid) {
        let mut ground = self.ground();
        for (x, y, variant) in terrain.iter() {
            ground
                .set_cell_ex(Vector2i::new(x as i32, y as i32))
                .source_id(0)
                .atlas_coords(Vector2i::new(i32::from(variant), 0))
                .done();
        }
        godot_print!(
            "[marrowfall] painted {}x{} ground tiles",
            terrain.width(),
            terrain.height()
        );
    }

    /// Centers the camera on the middle of the generated world.
    fn frame_camera(&self, terrain: &TerrainGrid) {
        let center = self.ground().map_to_local(Vector2i::new(
            (terrain.width() / 2) as i32,
            (terrain.height() / 2) as i32,
        ));
        self.base()
            .get_node_as::<Camera2D>(&self.camera_path)
            .set_position(center);
    }
}
