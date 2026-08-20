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
use std::sync::Arc;

use godot::classes::notify::NodeNotification;
use godot::classes::{Camera2D, FileAccess, Sprite2D, Texture2D, TileMapLayer, TileSet};
use godot::prelude::*;

use game::{Input, RenderSnapshot, Sim, TICK_DT};
use host::{ChunkMessage, SimHandle};
use sprites::CharacterAssets;

use crate::draw::{self, Clip};
use crate::iso;
use crate::origin::Origin;
use crate::tiles;

/// Fixed development seed; replaced by the new-game/save flow later.
const DEV_SEED: u64 = 0x4D61_7272_6F77; // "Marrow"

/// The only character there is, so every entity draws him.
const CHARACTER_DIR: &str = "res://assets/characters/survivor";

/// Where the tuning tables live. `res://` maps to `project/`, so these are the
/// files under `project/data`.
const DATA_DIR: &str = "res://data";

/// How many chunks out from the player stay resident, on each axis. A screen
/// shows about 400 tiles, so one chunk is roughly two and a half screens and a
/// radius of two keeps the edge of the world well off camera.
const RESIDENCY_RADIUS: u8 = 2;

#[derive(GodotClass)]
#[class(init, base=Node)]
pub struct GameBridge {
    base: Base<Node>,
    /// Y-sorted parent of every streamed chunk layer and every entity sprite.
    /// One parent, because Godot sorts within a subtree: ground and entities in
    /// separate parents can never interleave, so a cliff could never occlude a
    /// character.
    #[init(node = "../World")]
    world: OnReady<Gd<Node2D>>,
    /// Y-sorted parent of every entity sprite. A field and not a `base()`
    /// lookup, because `base_mut()` borrows all of `self`.

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
    /// One layer per chunk. Per chunk because `TileMapLayer` serialises a cell
    /// coordinate as an `i16`, so one layer for an endless world would wrap; and
    /// because freeing a chunk is then freeing one node.
    chunk_layers: HashMap<worldgen::ChunkCoord, Gd<TileMapLayer>>,
    /// The tileset every chunk layer shares.
    tile_set: Option<Gd<TileSet>>,
    announced: bool,
    /// Whether the first resident window has been reported. Streaming is silent
    /// once it works, so this is the one line that says it started.
    announced_world: bool,
    reported_death: bool,
    /// What every drawn position is expressed relative to. Without it, screen
    /// coordinates grow with distance until `f32` can no longer place a sprite
    /// to the nearest quarter pixel, which happens well inside this world.
    origin: Origin,
}

#[godot_api]
impl INode for GameBridge {
    fn ready(&mut self) {
        // No world means no collision, no residency and nothing to paint, so
        // this is fatal rather than something to draw around. A missing file and
        // an empty one are indistinguishable through `get_file_as_string`, which
        // is why the read is checked rather than trusted.
        let Some(rules) = self.load_rules() else {
            return;
        };
        let world = Arc::new(worldgen::World::new(rules, DEV_SEED));
        self.tile_set = try_load::<TileSet>("res://assets/tiles/ground.tileset.tres").ok();
        if self.tile_set.is_none() {
            godot_error!("[marrowfall] ground.tileset.tres missing; the world will not paint");
        }
        let sim = Sim::new();
        self.load_survivor();
        // Teardown is `SimHandle`'s own `Drop`, so the sim lives as long as
        // this node rather than as long as its membership of the tree.
        // `exit_tree` would also fire on a reparent, killing the sim while
        // `ready` never runs again.
        self.sim = Some(host::spawn(sim, world, RESIDENCY_RADIUS));
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

        // Before drawing, so every position this frame shares one origin. A
        // rebase mid-frame would put the camera and the sprites in different
        // frames of reference for exactly one frame, which reads as a jump.
        if let Some(player) = player_position(&snapshot, alpha)
            && self.origin.follow(player)
        {
            // Disjoint fields, so this cannot be a method: `sim` above is
            // already a mutable borrow of `self`.
            rebase_chunks(&mut self.chunk_layers, self.origin);
        }

        // Before drawing, so a chunk that arrived this frame is painted in the
        // same frame of reference as everything else.
        let messages = sim.take_chunks();
        self.apply_chunks(messages);

        let wanted = usize::from(RESIDENCY_RADIUS) * 2 + 1;
        if !self.announced_world && self.chunk_layers.len() >= wanted * wanted {
            self.announced_world = true;
            godot_print!(
                "[marrowfall] world streaming: {} chunks painted",
                self.chunk_layers.len()
            );
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
    /// Reads the tuning tables, or reports why the world cannot start.
    fn load_rules(&self) -> Option<worldgen::WorldRules> {
        let read = |name: &str| {
            let path = format!("{DATA_DIR}/{name}");
            let mut file = FileAccess::open(&path, godot::classes::file_access::ModeFlags::READ)?;
            let text = file.get_as_text().to_string();
            file.close();
            Some(text)
        };
        let names = [
            "world.tsv",
            "tiers.tsv",
            "materials.tsv",
            "biomes.tsv",
            "site_classes.tsv",
            "sites.tsv",
        ];
        let mut texts = Vec::with_capacity(names.len());
        for name in names {
            match read(name) {
                Some(text) => texts.push(text),
                None => {
                    godot_error!("[marrowfall] cannot read {DATA_DIR}/{name}; there is no world");
                    return None;
                }
            }
        }
        match worldgen::parse(worldgen::Tables {
            world: &texts[0],
            tiers: &texts[1],
            materials: &texts[2],
            biomes: &texts[3],
            site_classes: &texts[4],
            sites: &texts[5],
        }) {
            Ok(rules) => Some(rules),
            Err(error) => {
                godot_error!("[marrowfall] {error}");
                None
            }
        }
    }

    /// Creates, paints and frees one `TileMapLayer` per streamed chunk.
    fn apply_chunks(&mut self, messages: Vec<ChunkMessage>) {
        for message in messages {
            match message {
                ChunkMessage::Dropped(coord) => {
                    if let Some(mut layer) = self.chunk_layers.remove(&coord) {
                        layer.queue_free();
                    }
                }
                ChunkMessage::Ready(view) => self.paint_chunk(&view),
            }
        }
    }

    /// Paints one chunk, replacing any layer already at that coordinate.
    ///
    /// A coordinate can arrive twice if it left and re-entered residency, so the
    /// old node is freed first: two nodes with one name is a Godot error.
    fn paint_chunk(&mut self, view: &worldgen::ChunkView) {
        let Some(tile_set) = self.tile_set.clone() else {
            return;
        };
        if let Some(mut stale) = self.chunk_layers.remove(&view.coord) {
            stale.queue_free();
        }

        let mut layer = TileMapLayer::new_alloc();
        layer.set_name(&format!("chunk_{}_{}", view.coord.x, view.coord.y));
        layer.set_tile_set(&tile_set);
        // The simulation owns collision and pathfinding, and each of these is a
        // separate subsystem inside Godot's per change update, so leaving them on
        // would cost work for answers nothing reads.
        layer.set_collision_enabled(false);
        layer.set_navigation_enabled(false);
        layer.set_occlusion_enabled(false);
        layer.set_y_sort_enabled(true);
        // Local cell coordinates, so the layer itself carries the chunk's offset.
        layer.set_position(iso::chunk_to_screen(view.coord, self.origin));

        let data = tiles::tile_map_data(view);
        layer.set_tile_map_data_from_array(&PackedByteArray::from(data.as_slice()));

        self.world.add_child(&layer);
        self.chunk_layers.insert(view.coord, layer);
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
    fn draw_entities(&mut self, snapshot: &RenderSnapshot, alpha: f64) {
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
            self.world.add_child(&sprite);
            self.sprites.insert(id, sprite);
        }

        // `snapshot.time` stamps `pos`, but `lerp(0)` draws `prev_pos`, one
        // tick earlier. This walks back to the instant really drawn. Without it
        // the clip stays 16.7 ms ahead of the sprite for good.
        let seconds = snapshot.time - (1.0 - alpha) * TICK_DT;

        for view in &snapshot.entities {
            let Some(sprite) = self.sprites.get_mut(&view.id) else {
                continue;
            };
            sprite.set_position(iso::ground_to_screen(
                view.lerp(alpha),
                view.height,
                self.origin,
            ));

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
    fn follow_player(&mut self, snapshot: &RenderSnapshot, alpha: f64) {
        let Some(view) = snapshot
            .player
            .and_then(|id| snapshot.entities.iter().find(|view| view.id == id))
        else {
            return;
        };
        self.camera.set_global_position(iso::ground_to_screen(
            view.lerp(alpha),
            view.height,
            self.origin,
        ));
    }
}

/// Where the player is drawn this frame, or `None` when the snapshot has no
/// player to follow.
/// Moves every painted chunk to the new origin.
///
/// A layer's position is written once, when the chunk is painted, so nothing else
/// would move it. Entity sprites and the camera are placed fresh every frame and
/// rebase on their own, so without this terrain is left behind and drifts out of
/// alignment by however far the origin jumped.
fn rebase_chunks(layers: &mut HashMap<worldgen::ChunkCoord, Gd<TileMapLayer>>, origin: Origin) {
    for (coord, layer) in layers.iter_mut() {
        layer.set_position(iso::chunk_to_screen(*coord, origin));
    }
}

fn player_position(snapshot: &RenderSnapshot, alpha: f64) -> Option<game::WorldVec> {
    snapshot
        .player
        .and_then(|id| snapshot.entities.iter().find(|view| view.id == id))
        .map(|view| view.lerp(alpha))
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
