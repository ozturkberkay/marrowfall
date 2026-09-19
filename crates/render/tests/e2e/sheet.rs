//! Turning what Godot recorded into what a human looks at.

use std::path::Path;

use image::{RgbaImage, imageops};
use sprites::AnimationAtlas;

/// The one cell of a recorded frame the character is in.
///
/// The harness puts his ground anchor at the middle of the viewport, so the
/// cell's top left is that point less the atlas anchor. Cropping to it is
/// what makes a sheet of 92 frames readable instead of 92 wide shots of an
/// empty backdrop.
pub fn cell(recorded: &Path, atlas: &AnimationAtlas) -> RgbaImage {
    let frame = image::open(recorded)
        .unwrap_or_else(|error| panic!("reading {}: {error}", recorded.display()))
        .to_rgba8();
    let middle = (frame.width() / 2, frame.height() / 2);
    imageops::crop_imm(
        &frame,
        middle.0 - atlas.anchor.x,
        middle.1 - atlas.anchor.y,
        atlas.cell_width,
        atlas.cell_height,
    )
    .to_image()
}

/// Composites `images` into a grid `columns` wide, on a transparent field so
/// nothing but the frames themselves is judged.
pub fn grid(images: &[RgbaImage], columns: u32, dest: &Path) {
    assert!(columns > 0, "a grid needs at least one column");
    let width = images.iter().map(RgbaImage::width).max().unwrap_or(1);
    let height = images.iter().map(RgbaImage::height).max().unwrap_or(1);
    let rows = (images.len() as u32).div_ceil(columns);

    let mut sheet = RgbaImage::new(width * columns, height * rows);
    for (index, image) in images.iter().enumerate() {
        let index = index as u32;
        imageops::replace(
            &mut sheet,
            image,
            i64::from((index % columns) * width),
            i64::from((index / columns) * height),
        );
    }
    write(&sheet, dest);
}

/// Writes one image, creating the directory it goes in.
pub fn write(image: &RgbaImage, dest: &Path) {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|error| panic!("creating {}: {error}", parent.display()));
    }
    image
        .save(dest)
        .unwrap_or_else(|error| panic!("writing {}: {error}", dest.display()));
}
