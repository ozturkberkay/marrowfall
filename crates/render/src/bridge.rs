//! `GameBridge`: the single node through which the simulation reaches the
//! scene tree. It boots the sim, paints the ground the sim generated, and
//! consumes one snapshot per rendered frame. All Godot access stays on the
//! main thread.
//!
//! The tile-to-screen mapping lives in [`crate::iso`], and what to draw per
//! frame in [`crate::draw`]; both are pure, so this file is left with property
//! writes and node lifecycle, which is also why it is the one file no test
//! reaches.
//!
//! One `Sprite2D` per entity, with `centered = false` and `offset` carrying the
//! frame's own offset minus the animation's anchor, so the node origin is the
//! feet: the transform point and the y-sort key then coincide. The default
//! `centered = true` would sort by the sprite centre, which is head height and
//! differs per animation.
//!
//! `z_index` stays 0 everywhere, and the reason is the opposite of "Godot
//! ignores it inside a y-sorted parent": it *overrides* the sort. Items are
//! y-sorted and then bucketed by `z_index`, and the buckets draw in z order, so
//! a stray `z_index` silently defeats sorting.
//!
//! Sorting is settled for now: `Ground` is unsorted and `Entities` is a
//! y-sorted sibling listed after it, so entities land on top of flat ground,
//! which occludes nothing. Interleaving a character with terrain needs a
//! `y_sort_origin` per tile in the tileset, and waits for the first wall.
//!
//! Atlas bleeding is settled too, three times over: the pack stage writes a
//! two-pixel gutter between frames with mipmaps off,
//! `TileSetAtlasSource.use_texture_padding` covers the ground layer, and
//! `region_filter_clip_enabled` clips sampling to the region.
//!
//! A `host::Frame` borrows the handle for as long as it lives, so no method on
//! `GameBridge` can be called while one is alive. Copy out what is needed
//! first.

use std::collections::HashMap;

use godot::classes::{Camera2D, FileAccess, Sprite2D, Texture2D, TileMapLayer};
use godot::prelude::*;

use game::{RenderSnapshot, Sim, TICK_DT, TerrainGrid};
use host::SimHandle;
use sprites::CharacterAssets;

use crate::draw::{self, Clip};
use crate::iso;

/// Fixed development seed; replaced by the new-game/save flow later.
const DEV_SEED: u64 = 0x4D61_7272_6F77; // "Marrow"

/// Every entity draws the survivor, because he is the only character there is.
const CHARACTER_DIR: &str = "res://assets/characters/survivor";

#[derive(GodotClass)]
#[class(init, base=Node)]
pub struct GameBridge {
    base: Base<Node>,
    /// TileMapLayer the sim's ground terrain is painted into.
    #[init(node = "../Ground")]
    ground: OnReady<Gd<TileMapLayer>>,
    /// Y-sorted parent every entity's sprite hangs off. A field rather than a
    /// `base()` lookup, because `base_mut()` borrows all of `self`.
    #[init(node = "../Entities")]
    entities: OnReady<Gd<Node2D>>,
    /// Camera pinned to the controlled entity's drawn position.
    #[init(node = "../Camera2D")]
    camera: OnReady<Gd<Camera2D>>,
    sim: Option<SimHandle>,
    /// Atlas layout for every clip, or `None` when it could not be loaded, in
    /// which case nothing is drawn and the simulation still runs.
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
        self.frame_camera(sim.terrain());
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

        // Copy the tick out before touching anything else on `self`: the
        // methods below need all of it, and a `Frame` keeps the handle borrowed.
        let (snapshot, alpha) = {
            let frame = sim.read();
            (frame.snapshot.clone(), frame.alpha)
        };

        if !self.announced && snapshot.tick > 0 {
            self.announced = true;
            godot_print!("[marrowfall] sim thread live at tick {}", snapshot.tick);
        }

        self.draw_entities(&snapshot, alpha);
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

    /// Loads the manifest and one atlas texture per clip, all or nothing.
    ///
    /// A missing or invalid file is reported once and then draws nothing; the
    /// simulation is unaffected, the same way a dead sim thread is reported.
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

        // `snapshot.time` stamps `pos`, while `lerp(0)` draws `prev_pos`, one
        // tick earlier. Without walking back to the instant actually drawn the
        // clip runs 16.7 ms ahead of the sprite for good.
        let seconds = snapshot.time - f64::from(1.0 - alpha) * TICK_DT;

        for view in &snapshot.entities {
            let Some(sprite) = self.sprites.get_mut(&view.id) else {
                continue;
            };
            sprite.set_position(iso::tile_to_screen(view.lerp(alpha)));

            let clip = Clip::for_locomotion(view.locomotion);
            let (Some(atlas), Some(texture)) =
                (assets.animations.get(clip.name()), self.textures.get(&clip))
            else {
                continue;
            };
            // A row or cell this atlas lacks leaves the sprite on the frame it
            // had, which is the only sane answer mid-clip.
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
}

/// A sprite whose origin is the entity's tile, reading one frame out of an
/// atlas.
fn new_sprite() -> Gd<Sprite2D> {
    let mut sprite = Sprite2D::new_alloc();
    // With `centered` off, `offset` is the top left, which is what lets the
    // frame's own offset and the animation's anchor put the origin on the feet.
    sprite.set_centered(false);
    sprite.set_region_enabled(true);
    // Godot's own answer to atlas bleeding, over the pack stage's gutter.
    sprite.set_region_filter_clip_enabled(true);
    sprite
}
