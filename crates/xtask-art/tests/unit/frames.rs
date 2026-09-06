//! Synthetic bake frames: what a rendered set looks like, and each way one of
//! them can be wrong.
//!
//! The real 848 frames of the survivor are gitignored derived output, so the
//! `bake.*` negatives are built here. Every frame is centered on the canvas,
//! which is what makes the reflection two directions apart exact.

use std::path::Path;

use image::{Rgba, RgbaImage};

/// A square render canvas, small enough to build in a test and wide enough
/// for a 20 pixel offset to stay inside it.
pub const CANVAS: u32 = 64;

/// Where a good frame's content sits: horizontally centered, so the left
/// margin and the right margin are equal, and 12 px clear of every border.
const CONTENT: (u32, u32, u32, u32) = (24, 12, 16, 40);

/// One rendered frame, opaque where the character is.
pub fn a_frame() -> RgbaImage {
    filled(CONTENT.0, CONTENT.1, CONTENT.2, CONTENT.3)
}

/// The same content moved sideways, which is a frame drawn about another
/// pivot than the one the ring turns on.
pub fn a_frame_offset_by(pixels: u32) -> RgbaImage {
    filled(CONTENT.0 + pixels, CONTENT.1, CONTENT.2, CONTENT.3)
}

/// A pose the camera cut off: content against the right border.
pub fn a_clipped_frame() -> RgbaImage {
    filled(CANVAS - CONTENT.2, CONTENT.1, CONTENT.2, CONTENT.3)
}

/// A frame that drew nothing at all.
pub fn an_empty_frame() -> RgbaImage {
    RgbaImage::new(CANVAS, CANVAS)
}

fn filled(x: u32, y: u32, width: u32, height: u32) -> RgbaImage {
    let mut frame = RgbaImage::new(CANVAS, CANVAS);
    for row in y..y + height {
        for column in x..x + width {
            frame.put_pixel(column, row, Rgba([200, 170, 140, 255]));
        }
    }
    frame
}

/// Writes one clip's whole rendered set: every direction, every frame.
pub fn a_rendered_set(dir: &Path, name: &str, directions: &[&str], frames: u32) {
    std::fs::create_dir_all(dir).expect("mkdir");
    for direction in directions {
        for index in 0..frames {
            write_frame(dir, name, direction, index, &a_frame());
        }
    }
}

/// Where the bake writes one frame, and what `pack` reads back.
pub fn frame_path(dir: &Path, name: &str, direction: &str, index: u32) -> std::path::PathBuf {
    dir.join(format!("{name}_{direction}_{index:02}.png"))
}

pub fn write_frame(dir: &Path, name: &str, direction: &str, index: u32, frame: &RgbaImage) {
    frame
        .save(frame_path(dir, name, direction, index))
        .expect("writing a frame");
}
