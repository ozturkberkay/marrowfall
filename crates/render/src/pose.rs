//! The visual harness: one clip, one facing, one atlas frame per drawn frame.
//!
//! `project/scenes/pose.tscn` is its only scene, and the e2e tier drives it
//! with `--write-movie` so a human can judge what the player sees. It draws
//! through [`crate::draw`] and `sprites`, the two modules the game itself
//! draws through, and it reads no clock: frame `n` of the atlas is frame `n`
//! of the movie, which is what makes the sheets reproducible.

use godot::classes::Sprite2D;
use godot::prelude::*;

use game::Vec2;
use sprites::AnimationAtlas;

use crate::bridge::{CHARACTER_DIR, GameBridge, new_sprite};
use crate::draw::{self, Clip};

#[derive(GodotClass)]
#[class(init, base=Node2D)]
pub struct PoseHarness {
    base: Base<Node2D>,
    sprite: Option<Gd<Sprite2D>>,
    /// The clip being posed, or `None` when the arguments were not usable.
    atlas: Option<AnimationAtlas>,
    row: usize,
    frame: u32,
}

#[godot_api]
impl INode2D for PoseHarness {
    fn ready(&mut self) {
        let Some((clip, aim)) = arguments() else {
            self.refuse("expected `-- --clip=<name> --aim=<x>,<y>`");
            return;
        };
        let Some((assets, mut textures)) = GameBridge::load_character(CHARACTER_DIR) else {
            self.refuse(&format!("{CHARACTER_DIR} did not load"));
            return;
        };
        let (Some(atlas), Some(texture)) =
            (assets.animations.get(clip.name()), textures.remove(&clip))
        else {
            self.refuse(&format!("{CHARACTER_DIR} has no {} atlas", clip.name()));
            return;
        };
        let Some(row) = draw::row_for_aim(atlas, aim) else {
            self.refuse(&format!("no atlas row aims at {aim}"));
            return;
        };

        // The feet, because the node origin is the sprite's ground anchor.
        // The e2e tier crops each captured frame to the cell around this
        // point, so it has to be one both sides can name.
        let Some(viewport) = self.base().get_viewport() else {
            self.refuse("no viewport");
            return;
        };
        let center = viewport.get_visible_rect().size / 2.0;

        let mut sprite = new_sprite();
        sprite.set_texture(&texture);
        sprite.set_position(center);
        self.base_mut().add_child(&sprite);

        godot_print!(
            "[marrowfall] pose {} row {row} {} at {center}, {} frames",
            clip.name(),
            atlas.directions[row],
            atlas.frames
        );
        self.sprite = Some(sprite);
        self.row = row;
        self.atlas = Some(atlas.clone());
    }

    fn process(&mut self, _delta: f64) {
        let (Some(atlas), Some(sprite)) = (self.atlas.as_ref(), self.sprite.as_mut()) else {
            return;
        };
        // Nothing is set past the last frame, so the sprite holds still.
        // The caller's `--quit-after` stops the run there, and a held frame
        // reads as a stuck loop in the sheets if it does not.
        if let Some(rect) = sprites::frame(atlas, self.row, self.frame as usize) {
            let placement = draw::placement(atlas, rect);
            sprite.set_region_rect(placement.region);
            sprite.set_offset(placement.offset);
        }
        self.frame += 1;
    }
}

impl PoseHarness {
    /// Says why nothing can be posed and quits non-zero, so the caller does
    /// not sit through a run that draws an empty backdrop.
    fn refuse(&mut self, reason: &str) {
        godot_error!("[marrowfall] pose harness: {reason}");
        self.base().get_tree().quit_ex().exit_code(1).done();
    }
}

/// The clip and the facing, out of the arguments after Godot's own `--`.
fn arguments() -> Option<(Clip, Vec2)> {
    let mut clip = None;
    let mut aim = None;
    for argument in godot::classes::Os::singleton()
        .get_cmdline_user_args()
        .as_slice()
    {
        let argument = argument.to_string();
        if let Some(name) = argument.strip_prefix("--clip=") {
            clip = Clip::ALL.into_iter().find(|clip| clip.name() == name);
        } else if let Some(pair) = argument.strip_prefix("--aim=") {
            aim = parse_aim(pair);
        }
    }
    Some((clip?, aim?))
}

/// `<x>,<y>` in tile space, which is the space the atlas rows are spaced
/// evenly in.
fn parse_aim(pair: &str) -> Option<Vec2> {
    let (x, y) = pair.split_once(',')?;
    Some(Vec2::new(x.trim().parse().ok()?, y.trim().parse().ok()?))
}
