//! `GameBridge`: the single node through which the simulation reaches the
//! scene tree. It boots the sim, paints the ground the sim generated, and
//! consumes one snapshot per rendered frame. All Godot access stays on the
//! main thread.
//!
//! [`crate::iso`] holds the tile-to-screen mapping, and [`crate::draw`] picks
//! what to draw each frame. Both are pure, so this file keeps only property
//! writes and node lifecycle. That is also why no test reaches it.
//!
//! Each entity gets one `Sprite2D` with `centered = false`. Its `offset` is the
//! frame's own offset minus the animation's anchor, which puts the node origin
//! on the feet. The transform point and the y-sort key are then the same point.
//! With the default `centered = true` the sort key is the sprite centre, which
//! is head height and differs per animation.
//!
//! `z_index` stays 0 everywhere. Inside a y-sorted parent Godot does not ignore
//! `z_index`, it *overrides* the sort: items sort by y, then split into
//! `z_index` buckets, and the buckets draw in z order. One stray `z_index`
//! therefore defeats the sort.
//!
//! `Ground` is unsorted, and `Entities` is a y-sorted sibling listed after it.
//! So entities draw on top of flat ground, which occludes nothing. To
//! interleave a character with terrain, the tileset needs a `y_sort_origin` per
//! tile. That waits for the first wall.
//!
//! Three things stop atlas bleeding. The pack stage writes a two-pixel gutter
//! between frames, with mipmaps off. `TileSetAtlasSource.use_texture_padding`
//! covers the ground layer. `region_filter_clip_enabled` clips sampling to the
//! region.
//!
//! A `host::Frame` borrows the handle for as long as it lives, so no method on
//! `GameBridge` can be called while one is alive. Copy out what is needed
//! first.

use std::collections::HashMap;

use godot::classes::notify::NodeNotification;
use godot::classes::{Camera2D, FileAccess, Sprite2D, Texture2D, TileMapLayer};
use godot::prelude::*;

use game::{Input, RenderSnapshot, Sim, TICK_DT, TerrainGrid};
use host::SimHandle;
use sprites::CharacterAssets;

use crate::draw::{self, Clip};
use crate::iso;

/// Fixed development seed; replaced by the new-game/save flow later.
const DEV_SEED: u64 = 0x4D61_7272_6F77; // "Marrow"

/// The only character there is, so every entity draws him.
const CHARACTER_DIR: &str = "res://assets/characters/survivor";

#[derive(GodotClass)]
#[class(init, base=Node)]
pub struct GameBridge {
    base: Base<Node>,
    /// TileMapLayer the sim's ground terrain is painted into.
    #[init(node = "../Ground")]
    ground: OnReady<Gd<TileMapLayer>>,
    /// Y-sorted parent of every entity sprite. A field and not a `base()`
    /// lookup, because `base_mut()` borrows all of `self`.
    #[init(node = "../Entities")]
    entities: OnReady<Gd<Node2D>>,
    /// Camera pinned to the player's drawn position.
    #[init(node = "../Camera2D")]
    camera: OnReady<Gd<Camera2D>>,
    /// Whether the window has keyboard focus. It starts `true` because the
    /// window already has focus when this node enters the tree. No focus
    /// notification arrives in that case, and waiting for one leaves WASD dead.
    #[init(val = true)]
    focused: bool,
    sim: Option<SimHandle>,
    /// Atlas layout for every clip, or `None` when the load failed. A failed
    /// load draws nothing and leaves the simulation running.
    assets: Option<CharacterAssets>,
    textures: HashMap<Clip, Gd<Texture2D>>,
    sprites: HashMap<u64, Gd<Sprite2D>>,
    announced: bool,
    reported_death: bool,
}

#[godot_api]
impl INode for GameBridge {
    fn ready(&mut self) {
        let sim = Sim::new(DEV_SEED);
        self.paint_ground(sim.terrain());
        self.load_survivor();
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

        // Once per frame, before `read` borrows the handle. The simulation
        // reads it once per tick, so speed never tracks frame rate.
        let held = iso::screen_dir_to_tile(held_direction(self.focused));
        sim.set_input(Input::new(held));

        // Copy the snapshot out first: the methods below need all of `self`,
        // and a `Frame` keeps the handle borrowed.
        let (snapshot, alpha) = {
            let frame = sim.read();
            (frame.snapshot.clone(), frame.alpha)
        };

        if !self.announced && snapshot.tick > 0 {
            self.announced = true;
            godot_print!("[marrowfall] sim thread live at tick {}", snapshot.tick);
        }

        self.draw_entities(&snapshot, alpha);
        self.follow_player(&snapshot, alpha);
    }

    /// The OS does not always send a key release when the window loses focus,
    /// so a held key stays held forever. The fix is to gate the sample on
    /// `focused`. A still input written from here alone loses to the sample
    /// taken in the same frame.
    fn on_notification(&mut self, what: NodeNotification) {
        match what {
            NodeNotification::WM_WINDOW_FOCUS_IN => self.focused = true,
            NodeNotification::WM_WINDOW_FOCUS_OUT => self.focused = false,
            _ => {}
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

    /// Loads the manifest and one atlas texture per clip, all or nothing.
    ///
    /// A missing or invalid file is reported once and then draws nothing. The
    /// simulation runs on, the same way it does when the sim thread dies.
    fn load_survivor(&mut self) {
        let manifest = format!("{CHARACTER_DIR}/character.ron");
        let assets = match sprites::parse(&FileAccess::get_file_as_string(&manifest).to_string()) {
            Ok(assets) => assets,
            Err(error) => {
                godot_error!("[marrowfall] {manifest}: {error}");
                return;
            }
        };

        let mut textures = HashMap::new();
        for clip in Clip::ALL {
            let Some(atlas) = assets.animations.get(clip.name()) else {
                godot_error!("[marrowfall] {manifest} has no {} animation", clip.name());
                return;
            };
            let path = format!("{CHARACTER_DIR}/{}", atlas.file);
            match try_load::<Texture2D>(&path) {
                Ok(texture) => textures.insert(clip, texture),
                Err(error) => {
                    godot_error!("[marrowfall] {path}: {error}");
                    return;
                }
            };
        }

        self.textures = textures;
        self.assets = Some(assets);
    }

    /// Creates, moves and frees one `Sprite2D` per entity in the snapshot.
    fn draw_entities(&mut self, snapshot: &RenderSnapshot, alpha: f32) {
        let Some(assets) = self.assets.as_ref() else {
            return;
        };

        let changes = draw::reconcile(&snapshot.entities, self.sprites.keys().copied());
        for id in changes.removed {
            if let Some(mut sprite) = self.sprites.remove(&id) {
                sprite.queue_free();
            }
        }
        for id in changes.added {
            let sprite = new_sprite();
            self.entities.add_child(&sprite);
            self.sprites.insert(id, sprite);
        }

        // `snapshot.time` stamps `pos`, but `lerp(0)` draws `prev_pos`, one
        // tick earlier. This walks back to the instant really drawn. Without it
        // the clip stays 16.7 ms ahead of the sprite for good.
        let seconds = snapshot.time - (1.0 - f64::from(alpha)) * TICK_DT;

        for view in &snapshot.entities {
            let Some(sprite) = self.sprites.get_mut(&view.id) else {
                continue;
            };
            sprite.set_position(iso::tile_to_screen(view.lerp(alpha)));

            // A missing clip, row or frame leaves the sprite as it is, which is
            // the only sane answer mid-clip.
            let clip = Clip::for_locomotion(view.locomotion);
            let Some(atlas) = assets.animations.get(clip.name()) else {
                continue;
            };
            let Some(texture) = self.textures.get(&clip) else {
                continue;
            };
            let Some(row) = sprites::row_for(atlas, view.facing.name()) else {
                continue;
            };
            let Some(rect) = sprites::frame(atlas, row, sprites::frame_at(atlas, seconds)) else {
                continue;
            };

            let placement = draw::placement(atlas, rect);
            sprite.set_texture(texture);
            sprite.set_region_rect(placement.region);
            sprite.set_offset(placement.offset);
        }
    }

    /// Pins the camera to the player's drawn position, so he is rock steady and
    /// every residual error lands on the terrain instead. That is the cheapest
    /// place to put it.
    ///
    /// This runs in `_process` on purpose. `SceneTree::process` flushes
    /// transform notifications straight after it, and `Camera2D` updates its
    /// scroll from that notification. Position smoothing and physics
    /// interpolation both stay off. Either one adds a frame of lag, and is a
    /// second interpolator that argues with `host`'s `alpha`.
    fn follow_player(&mut self, snapshot: &RenderSnapshot, alpha: f32) {
        let Some(view) = snapshot
            .player
            .and_then(|id| snapshot.entities.iter().find(|view| view.id == id))
        else {
            return;
        };
        self.camera
            .set_global_position(iso::tile_to_screen(view.lerp(alpha)));
    }
}

/// Which way the movement keys point on screen, or nowhere when the window is
/// not focused.
fn held_direction(focused: bool) -> Vector2 {
    if !focused {
        return Vector2::ZERO;
    }
    // `get_vector` applies the deadzone and clamps to length 1. A gamepad will
    // need that, and a keyboard does not mind it.
    godot::classes::Input::singleton().get_vector("move_left", "move_right", "move_up", "move_down")
}

/// A sprite that draws one frame out of an atlas, with its origin on the
/// entity's tile.
fn new_sprite() -> Gd<Sprite2D> {
    let mut sprite = Sprite2D::new_alloc();
    sprite.set_centered(false);
    sprite.set_region_enabled(true);
    // Godot's own guard against atlas bleeding, on top of the pack gutter.
    sprite.set_region_filter_clip_enabled(true);
    sprite
}
