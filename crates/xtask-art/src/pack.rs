//! Turns loose bake frames into game-ready sprite atlases. Three things are
//! shared, and each is easy to destroy:
//!
//! 1. **One crop per animation**, or the sprite jitters between frames.
//! 2. **One scale per character**, or he shrinks when he starts running.
//! 3. **Scale keyed to the first animation**, so `sprite_height` means
//!    standing height and two characters authored alike match.
//!
//! Cell size is *not* shared; the per-animation [`Anchor`] lines them up.

use std::path::Path;

use anyhow::{Context as _, Result, bail};
use image::{RgbaImage, imageops};

/// The manifest format lives in `sprites`, because the game reads the same
/// file. Re-exported so this module still reads as one pipeline stage.
pub use sprites::{Anchor, AnimationAtlas, CharacterAssets, FrameRect};

/// Compass directions in the order the Blender bake writes them. Index 0 faces
/// the camera, and the model turns clockwise from there. So the ring runs
/// south, south-west, west, and on round. Must match `DIRECTION_NAMES` in
/// `tools/blender/src/framing.py`.
const DIRECTIONS_8: [&str; 8] = ["s", "sw", "w", "nw", "n", "ne", "e", "se"];
const DIRECTIONS_4: [&str; 4] = ["s", "w", "n", "e"];

/// Direction names for an evenly spaced ring.
pub fn direction_names(count: u32) -> Result<&'static [&'static str]> {
    match count {
        8 => Ok(&DIRECTIONS_8),
        4 => Ok(&DIRECTIONS_4),
        other => bail!("unsupported direction count {other}; expected 4 or 8"),
    }
}

/// A rectangle of pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = (self.x + self.width).max(other.x + other.width);
        let bottom = (self.y + self.height).max(other.y + other.height);
        Self {
            x,
            y,
            width: right - x,
            height: bottom - y,
        }
    }

    /// Whether the rectangle touches the edge of a `width` x `height` canvas,
    /// which means the render was clipped and the camera is framed too tight.
    fn touches_border(self, width: u32, height: u32) -> bool {
        self.x == 0 || self.y == 0 || self.x + self.width >= width || self.y + self.height >= height
    }
}

/// Smallest rectangle containing every pixel at or above `alpha_threshold`.
pub fn content_bounds(image: &RgbaImage, alpha_threshold: u8) -> Option<Rect> {
    let (mut min_x, mut min_y) = (u32::MAX, u32::MAX);
    let (mut max_x, mut max_y) = (0_u32, 0_u32);
    let mut found = false;

    for (x, y, pixel) in image.enumerate_pixels() {
        if pixel.0[3] >= alpha_threshold {
            found = true;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }

    found.then(|| Rect {
        x: min_x,
        y: min_y,
        width: max_x - min_x + 1,
        height: max_y - min_y + 1,
    })
}

/// The scale shared by every animation of one character. Only scale and the
/// rotation axis are shared; each animation crops to its own content, and the
/// per-animation anchor is what lines them up.
#[derive(Debug, Clone, Copy)]
pub struct CharacterScale {
    scale: f64,
    /// Rotation axis in render-canvas pixels.
    axis_x: u32,
    /// Canvas row the character stands on, taken from the reference animation.
    /// Shared so an airborne animation is not planted on the tile as if grounded.
    ground_y: u32,
}

/// One frame plus where it came from.
pub struct Frame {
    pub direction: &'static str,
    pub index: u32,
    pub image: RgbaImage,
}

/// Loads every frame of `animation`, named `<animation>_<direction>_<NN>.png`.
pub fn load_animation_frames(
    dir: &Path,
    animation: &str,
    directions: &'static [&'static str],
) -> Result<Vec<Frame>> {
    let mut frames = Vec::new();
    for direction in directions {
        let mut index = 0;
        loop {
            let path = dir.join(format!("{animation}_{direction}_{index:02}.png"));
            if !path.exists() {
                // A gap mid-sequence means the bake died partway; treating it
                // as the end would silently ship a truncated animation.
                let next = dir.join(format!("{animation}_{direction}_{:02}.png", index + 1));
                anyhow::ensure!(
                    !next.exists(),
                    "frame {index:02} of animation {animation:?} direction {direction:?} is \
                     missing but later frames exist, the bake did not finish. \
                     Re-run with --from bake."
                );
                break;
            }
            let image = image::open(&path)
                .with_context(|| format!("reading frame {}", path.display()))?
                .to_rgba8();
            frames.push(Frame {
                direction,
                index,
                image,
            });
            index += 1;
        }
        if index == 0 {
            bail!(
                "no frames for animation {animation:?} direction {direction:?} in {}, did the bake stage run?",
                dir.display()
            );
        }
    }
    Ok(frames)
}

/// Union of the content bounds across frames, plus any that were clipped.
pub fn union_bounds(frames: &[Frame]) -> (Option<Rect>, Vec<String>) {
    let mut union: Option<Rect> = None;
    let mut clipped = Vec::new();
    for frame in frames {
        let (width, height) = (frame.image.width(), frame.image.height());
        let Some(bounds) = content_bounds(&frame.image, 1) else {
            continue;
        };
        if bounds.touches_border(width, height) {
            clipped.push(format!("{}_{:02}", frame.direction, frame.index));
        }
        union = Some(match union {
            Some(existing) => existing.union(bounds),
            None => bounds,
        });
    }
    (union, clipped)
}

/// Derives the scale from the first animation, so `sprite_height` means
/// standing height. Errors if any pose was clipped: the camera was framed too
/// tightly and packing cannot repair it.
pub fn character_scale<'a>(
    animations: impl IntoIterator<Item = &'a [Frame]>,
    sprite_height: u32,
) -> Result<CharacterScale> {
    let mut reference: Option<Rect> = None;
    let mut clipped = Vec::new();
    let mut canvas_width = 0;

    for (index, frames) in animations.into_iter().enumerate() {
        if let Some(first) = frames.first() {
            canvas_width = first.image.width();
        }
        let (bounds, mut frame_clipped) = union_bounds(frames);
        clipped.append(&mut frame_clipped);
        if index == 0 {
            reference = bounds;
        }
    }

    anyhow::ensure!(
        clipped.is_empty(),
        "{} frame(s) are clipped by the render canvas (e.g. {}). \
         The bake camera is framed too tightly, widen it and re-bake.",
        clipped.len(),
        clipped
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    );
    let reference = reference.context("the reference animation had no visible frames")?;
    anyhow::ensure!(canvas_width > 0, "no frames supplied");

    Ok(CharacterScale {
        scale: f64::from(sprite_height) / f64::from(reference.height),
        ground_y: reference.y + reference.height - 1,
        // The bake centres the camera on the character, so the canvas's
        // horizontal centre is his axis of rotation. That is more stable than
        // any crop's centre, which drifts when a limb swings out to one side.
        axis_x: canvas_width / 2,
    })
}

/// Packs one animation into an atlas: a row per direction, a column per
/// frame. Frames share one crop, so the character cannot jitter.
/// Gutter between packed frames. Without it bilinear filtering samples a
/// neighbouring frame at the seam. Two pixels is enough with mipmaps off.
const GUTTER: u32 = 2;

/// Block-compressed textures are stored in 4x4 blocks, so Godot pads the atlas
/// out to that grid but keeps computing UVs from the unpadded size. Sizing to
/// the grid ourselves means padded and real are the same thing.
const fn align_to_block(value: u32) -> u32 {
    value.div_ceil(4) * 4
}

/// Shelf-packs `sizes`, returning a position for each and the atlas size.
///
/// Deterministic: tallest first, ties broken on the original index, so the same
/// frames always pack the same way.
fn shelf_pack(sizes: &[(u32, u32)]) -> (Vec<(u32, u32)>, u32, u32) {
    let area: u64 = sizes
        .iter()
        .map(|(w, h)| u64::from(w + GUTTER) * u64::from(h + GUTTER))
        .sum();
    let widest = sizes.iter().map(|(w, _)| w + GUTTER).max().unwrap_or(1);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let square = ((area as f64).sqrt() * 1.05).ceil() as u32;
    let target = square.max(widest);

    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(sizes[i].1), i));

    let mut placed = vec![(0, 0); sizes.len()];
    let (mut x, mut y, mut shelf_height, mut used_width) = (0, 0, 0, 0);
    for &i in &order {
        let (w, h) = sizes[i];
        if x > 0 && x + w + GUTTER > target {
            y += shelf_height;
            x = 0;
            shelf_height = 0;
        }
        placed[i] = (x, y);
        x += w + GUTTER;
        shelf_height = shelf_height.max(h + GUTTER);
        used_width = used_width.max(x);
    }
    (
        placed,
        align_to_block(used_width),
        align_to_block(y + shelf_height),
    )
}

/// Writes Godot's import settings beside an atlas.
///
/// BC7 rather than the default S3TC, which is the same 8 bits per pixel at half
/// the error. This is scoped to character atlases deliberately: Godot's own
/// advice is to leave 2D lossless, and that advice is right for tiles and UI,
/// which are small and full of hard edges. These are megapixel continuous-tone
/// renders, the case block compression was built for.
///
/// `detect_3d/compress_to=0` stops Godot quietly rewriting these if an atlas is
/// ever seen in a 3D context.
///
/// Only the settings above are written. Godot restores the content hash,
/// `[deps]` and every remaining default byte for byte on the next import, so
/// omitting them loses nothing. `uid` is the exception: it is minted fresh
/// whenever it is absent, so an existing one is carried across and re-packing an
/// unchanged atlas leaves the file exactly as it was.
pub fn write_import_settings(atlas: &Path) -> Result<()> {
    let path = atlas.with_extension("png.import");
    let uid = existing_uid(&path).map_or_else(String::new, |line| format!("{line}\n"));
    std::fs::write(
        &path,
        format!(
            "[remap]\n\n\
             importer=\"texture\"\n\
             type=\"CompressedTexture2D\"\n\
             {uid}\n\
             [params]\n\n\
             compress/mode=2\n\
             compress/high_quality=true\n\
             mipmaps/generate=false\n\
             process/fix_alpha_border=true\n\
             detect_3d/compress_to=0\n"
        ),
    )
    .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// The `uid` line of an import file already on disk, if it has one. Absent file,
/// unreadable file and no `uid` line are all the same answer: nothing to keep.
fn existing_uid(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .find(|line| line.starts_with("uid=\"uid://"))
        .map(str::to_owned)
}

pub fn pack_animation(
    frames: &[Frame],
    directions: &'static [&'static str],
    file: String,
    fps: u32,
    loops: bool,
    character: &CharacterScale,
) -> Result<(RgbaImage, AnimationAtlas)> {
    anyhow::ensure!(!frames.is_empty(), "cannot pack an empty animation");

    let frames_per_direction = frames
        .iter()
        .filter(|frame| frame.direction == directions[0])
        .count() as u32;
    anyhow::ensure!(
        frames_per_direction > 0,
        "no frames for the first direction"
    );
    for direction in directions {
        let count = frames.iter().filter(|f| f.direction == *direction).count() as u32;
        anyhow::ensure!(
            count == frames_per_direction,
            "direction {direction:?} has {count} frames but {:?} has {frames_per_direction}",
            directions[0]
        );
    }

    let (crop, _) = union_bounds(frames);
    let crop = crop.context("every frame in this animation is fully transparent")?;
    anyhow::ensure!(
        character.axis_x >= crop.x && character.axis_x < crop.x + crop.width,
        "the character's rotation axis (x={}) falls outside this animation's content \
         crop ({}..{}), the bake camera is not centred on him, so the sprite \
         would not line up with its tile",
        character.axis_x,
        crop.x,
        crop.x + crop.width
    );

    let ground_offset = character.ground_y.checked_sub(crop.y).with_context(|| {
        format!(
            "this animation's content starts at y={}, below the character's ground line \
             (y={}) taken from the reference animation. The reference animation is not standing \
             on the ground, so nothing can be aligned to it.",
            crop.y, character.ground_y
        )
    })?;
    let cell_width = ((f64::from(crop.width) * character.scale).round() as u32).max(1);
    let cell_height = ((f64::from(crop.height) * character.scale).round() as u32).max(1);

    // Scale each frame into the shared cell, then trim it to its own content.
    // The cell stays the coordinate system the anchor is expressed in; trimming
    // only decides which pixels are worth storing.
    let slots = frames_per_direction as usize * directions.len();
    let mut cells: Vec<Option<(RgbaImage, u32, u32)>> = vec![None; slots];
    for frame in frames {
        let row = directions
            .iter()
            .position(|d| *d == frame.direction)
            .with_context(|| format!("frame has unknown direction {:?}", frame.direction))?
            as u32;

        let cropped =
            imageops::crop_imm(&frame.image, crop.x, crop.y, crop.width, crop.height).to_image();
        let scaled = imageops::resize(
            &cropped,
            cell_width,
            cell_height,
            imageops::FilterType::Lanczos3,
        );
        // A fully transparent frame still needs a slot, so give it one pixel.
        let bounds = content_bounds(&scaled, 1).unwrap_or(Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        });
        let trimmed =
            imageops::crop_imm(&scaled, bounds.x, bounds.y, bounds.width, bounds.height).to_image();
        let slot = (row * frames_per_direction + frame.index) as usize;
        anyhow::ensure!(
            slot < slots,
            "frame {} of {:?} is outside this animation's {slots} slots",
            frame.index,
            frame.direction
        );
        cells[slot] = Some((trimmed, bounds.x, bounds.y));
    }
    let cells = cells
        .into_iter()
        .enumerate()
        .map(|(slot, cell)| cell.with_context(|| format!("no frame packed into slot {slot}")))
        .collect::<Result<Vec<_>>>()?;

    let sizes: Vec<(u32, u32)> = cells
        .iter()
        .map(|(image, _, _)| (image.width(), image.height()))
        .collect();
    let (positions, atlas_width, atlas_height) = shelf_pack(&sizes);

    let mut atlas = RgbaImage::new(atlas_width, atlas_height);
    let mut rects = Vec::with_capacity(cells.len());
    for ((image, off_x, off_y), (x, y)) in cells.iter().zip(&positions) {
        imageops::replace(&mut atlas, image, i64::from(*x), i64::from(*y));
        rects.push(FrameRect {
            x: *x,
            y: *y,
            w: image.width(),
            h: image.height(),
            off_x: *off_x,
            off_y: *off_y,
        });
    }

    let layout = AnimationAtlas {
        file,
        directions: directions.iter().map(|d| (*d).to_owned()).collect(),
        frames: frames_per_direction,
        fps,
        loops,
        cell_width,
        cell_height,
        anchor: Anchor {
            x: ((f64::from(character.axis_x - crop.x)) * character.scale).round() as u32,
            // The shared ground line, expressed in this animation's cell. For a
            // grounded animation this is the bottom row; for an airborne one it
            // sits below the feet, which is where the tile actually is.
            y: ((f64::from(ground_offset)) * character.scale).round() as u32,
        },
        rects,
    };
    Ok((atlas, layout))
}
