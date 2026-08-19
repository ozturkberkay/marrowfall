//! `GameBridge`: the single node through which the simulation reaches the
//! scene tree. It boots the sim, paints the ground the sim generated, and
//! consumes one snapshot per rendered frame. All Godot access stays on the
//! main thread.
//!
//! Tile space to screen space, measured against Godot's isometric
//! `TileMapLayer` rather than derived: for 192x96 diamond-down tiles,
//! `screen = ((x - y) * 96 + 96, (x + y) * 48 + 48)`. Two consequences the
//! rest of the frontend depends on. Both tile axes run *down* the screen, `+x`
//! to the right and `+y` to the left, so the eight sprite rows the art
//! pipeline bakes must be chosen by quantising the angle in **tile** space:
//! equal 45 degree sectors there are unequal on screen. And screen depth is
//! `x + y`, which is the only sort key isometric occlusion needs.
//!
//! Sprites will hang off a y-sorted `Node2D`, with `centered = false` and
//! `offset = -anchor` so the node origin is the feet: the transform point and
//! the sort key then coincide. The default `centered = true` would sort by the
//! sprite centre, which is head height and differs per animation. `z_index`
//! stays 0, since Godot 4 ignores it inside a y-sorted parent. Two things to
//! settle then: the tile layer wants to be in the same sorted tree or entities
//! always draw over terrain, unfixable once there is a wall; and atlas cells
//! are packed edge to edge under the default linear filter, so a sub-pixel
//! position can bleed the neighbouring frame, fixed by a gutter in the pack
//! stage or by a nearest filter.
//!
//! A `host::Frame` borrows the handle for as long as it lives, so no method on
//! `GameBridge` can be called while one is alive. Copy out what is needed
//! first.

use godot::classes::{Camera2D, TileMapLayer};
use godot::prelude::*;

use game::{Sim, TerrainGrid};
use host::SimHandle;

/// Fixed development seed; replaced by the new-game/save flow later.
const DEV_SEED: u64 = 0x4D61_7272_6F77; // "Marrow"

#[derive(GodotClass)]
#[class(init, base=Node)]
pub struct GameBridge {
    base: Base<Node>,
    /// TileMapLayer the sim's ground terrain is painted into.
    #[init(node = "../Ground")]
    ground: OnReady<Gd<TileMapLayer>>,
    /// Camera framed on the generated world at startup.
    #[init(node = "../Camera2D")]
    camera: OnReady<Gd<Camera2D>>,
    sim: Option<SimHandle>,
    announced: bool,
    reported_death: bool,
}

#[godot_api]
impl INode for GameBridge {
    fn ready(&mut self) {
        let sim = Sim::new(DEV_SEED);
        self.paint_ground(sim.terrain());
        self.frame_camera(sim.terrain());
        // Teardown is `SimHandle`'s own `Drop`, so the sim lives as long as
        // this node rather than as long as its membership of the tree.
        // `exit_tree` would also fire on a reparent, killing the sim while
        // `ready` never runs again.
        self.sim = Some(host::spawn(sim));
    }

    fn process(&mut self, _delta: f64) {
        let Some(sim) = self.sim.as_mut() else {
            return;
        };

        if !sim.is_alive() {
            if !self.reported_death {
                self.reported_death = true;
                godot_error!("[marrowfall] simulation thread died; the world is frozen");
            }
            return;
        }

        // Entity nodes will be created, moved and freed from the snapshot here
        // once the world has entities. An entity absent from a snapshot is
        // gone, and its node must go with it.
        //
        // Read scalars out before touching anything else on `self`: any
        // `base()` call needed to parent a node borrows the whole struct, so a
        // snapshot borrow cannot still be alive.
        let tick = sim.read().snapshot.tick;

        if !self.announced && tick > 0 {
            self.announced = true;
            godot_print!("[marrowfall] sim thread live at tick {tick}");
        }
    }
}

impl GameBridge {
    /// Paints the sim's ground grid into the TileMapLayer: variant `n` maps
    /// to atlas column `n` of source 0 (see `ground.tileset.tres`).
    fn paint_ground(&mut self, terrain: &TerrainGrid) {
        for (x, y, variant) in terrain.iter() {
            self.ground
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
    fn frame_camera(&mut self, terrain: &TerrainGrid) {
        let middle = Vector2i::new((terrain.width() / 2) as i32, (terrain.height() / 2) as i32);
        // `map_to_local` answers in the layer's own space, so go through global
        // space rather than assuming the two nodes share an origin.
        let center = self.ground.to_global(self.ground.map_to_local(middle));
        self.camera.set_global_position(center);
    }
}
