//! Reviewable artefacts, one per stage.
//!
//! Every stage that produces something judgeable writes a preview here. This
//! is the only thing standing between a bad generation and a character built
//! on top of it, so previews show *shipped* pixels wherever possible rather
//! than intermediate ones.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use image::{RgbaImage, imageops};

use crate::pack::CharacterAssets;
use crate::spec::Paths;
use crate::spec::View;

/// Dark ground, so sprites are judged against the background they will
/// actually sit on rather than white.
pub const BACKDROP: image::Rgba<u8> = image::Rgba([58, 54, 46, 255]);

/// The four concept views side by side.
pub fn concept(paths: &Paths) -> Result<()> {
    let images: Vec<RgbaImage> = View::ALL
        .iter()
        .map(|view| paths.concept(*view))
        .filter(|path| path.exists())
        .map(|path| image::open(&path).map(|image| image.to_rgba8()))
        .collect::<Result<_, _>>()?;
    if images.is_empty() {
        return Ok(());
    }
    write_grid(
        &images,
        images.len() as u32,
        &paths.preview().join("concept.png"),
    )
}

/// The provider's own turntable renders, so the mesh is reviewable without
/// downloading it.
pub fn model(paths: &Paths, thumbnails: &[Vec<u8>]) -> Result<()> {
    let images: Vec<RgbaImage> = thumbnails
        .iter()
        .filter_map(|bytes| image::load_from_memory(bytes).ok())
        .map(|image| image.to_rgba8())
        .collect();
    if images.is_empty() {
        return Ok(());
    }
    write_grid(
        &images,
        images.len() as u32,
        &paths.preview().join("model.png"),
    )
}

/// Every animation across every direction, straight off the bake.
///
/// Read from the loose frames rather than the atlases, so the render can be
/// judged before packing decides anything about crops or scale.
pub fn bake(names: &[&str], directions: &[&str], paths: &Paths) -> Result<()> {
    let mut rows: Vec<RgbaImage> = Vec::new();
    for name in names {
        let mut cells: Vec<RgbaImage> = Vec::new();
        for direction in directions {
            // A mid-animation frame: retargeting failures hide at frame 0.
            let frames = frames_for(&paths.staging(), name, direction);
            if let Some(path) = frames.get(frames.len() / 3)
                && let Ok(frame) = image::open(path)
            {
                cells.push(frame.to_rgba8());
            }
        }
        if cells.is_empty() {
            continue;
        }
        let (w, h) = (cells[0].width(), cells[0].height());
        let mut row = RgbaImage::from_pixel(w * cells.len() as u32, h, BACKDROP);
        for (i, cell) in cells.iter().enumerate() {
            imageops::overlay(&mut row, cell, i64::from(w * i as u32), 0);
        }
        rows.push(row);
    }
    if rows.is_empty() {
        return Ok(());
    }
    write_grid(&rows, 1, &paths.preview().join("bake.png"))
}

/// Every frame of one animation facing one direction, in playback order.
fn frames_for(dir: &Path, name: &str, direction: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&format!("{name}_{direction}_")))
        })
        .collect();
    found.sort();
    found
}

/// Every animation × every direction at final sprite size, read from the packed
/// atlases so what you review is exactly what ships.
pub fn sprites(assets: &CharacterAssets, paths: &Paths) -> Result<()> {
    let mut rows: Vec<RgbaImage> = Vec::new();
    for animation in assets.animations.values() {
        let path = paths.assets().join(&animation.file);
        if !path.exists() {
            continue;
        }
        let atlas = image::open(&path)
            .with_context(|| format!("reading atlas {}", path.display()))?
            .to_rgba8();
        let directions = animation.directions.len() as u32;

        // A mid-animation frame, not frame 0: retargeting failures show up in the
        // middle of a motion, where frame 0 still looks like a clean bind pose.
        let column = animation.frames / 2;
        let mut row = RgbaImage::from_pixel(
            animation.cell_width * directions,
            animation.cell_height,
            BACKDROP,
        );
        for direction in 0..directions {
            // The atlas is shelf-packed and every frame trimmed to its own
            // content, so a row-and-column guess lands on whatever happens to
            // sit there. The recorded rect is where the frame actually is, and
            // `off_x`/`off_y` put those pixels back where the cell wants them.
            let Some(rect) = animation
                .rects
                .get((direction * animation.frames + column) as usize)
            else {
                continue;
            };
            let trimmed = imageops::crop_imm(&atlas, rect.x, rect.y, rect.w, rect.h).to_image();
            let mut cell = RgbaImage::new(animation.cell_width, animation.cell_height);
            imageops::replace(
                &mut cell,
                &trimmed,
                i64::from(rect.off_x),
                i64::from(rect.off_y),
            );
            imageops::overlay(
                &mut row,
                &cell,
                i64::from(direction * animation.cell_width),
                0,
            );
        }
        rows.push(row);
    }
    if rows.is_empty() {
        return Ok(());
    }
    write_grid(&rows, 1, &paths.preview().join("sprites.png"))
}

/// Composites images into a grid `columns` wide on the backdrop.
pub fn write_grid(images: &[RgbaImage], columns: u32, dest: &Path) -> Result<()> {
    anyhow::ensure!(columns > 0, "grid needs at least one column");
    let cell_width = images.iter().map(RgbaImage::width).max().unwrap_or(1);
    let cell_height = images.iter().map(RgbaImage::height).max().unwrap_or(1);
    let rows = (images.len() as u32).div_ceil(columns);

    let mut sheet = RgbaImage::from_pixel(cell_width * columns, cell_height * rows, BACKDROP);
    for (index, image) in images.iter().enumerate() {
        let index = index as u32;
        imageops::overlay(
            &mut sheet,
            image,
            i64::from((index % columns) * cell_width),
            i64::from((index / columns) * cell_height),
        );
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    sheet
        .save(dest)
        .with_context(|| format!("writing {}", dest.display()))?;
    println!("  preview → {}", dest.display());
    Ok(())
}
