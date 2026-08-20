//! The terraced height field.
//!
//! Ground is flat within a terrace and vertical between terraces, so the field
//! is an integer number of steps rather than a continuous surface. That is what
//! makes cliff art a fixed handful of pieces instead of nineteen slope variants
//! per material.

use glam::IVec2;

use crate::region::region_at;
use crate::rules::HEIGHT_RANGE;
use crate::world::World;

/// Surface level at `tile`, in whole steps.
#[must_use]
pub fn height_at(world: &World, tile: IVec2) -> i8 {
    let region = region_at(world, tile);
    let biome = world.rules().biome(region.biome);
    // The table holds a period in tiles ("features about 140 tiles across")
    // rather than a noise frequency, so a designer can reason about it. Integer
    // tile coordinates are exact in f32 up to 2^24, which is far beyond where
    // the world stops being playable, so f64 positions never reach this API.
    let period = biome.height_period as f32;
    let n = world
        .height_noise()
        .get_noise_2d(tile.x as f32 / period, tile.y as f32 / period);
    // Quantise here. Nothing float valued is ever stored, so a one bit rounding
    // difference on another platform cannot change a world.
    let steps = (n * f32::from(biome.height_amp)).round();
    // `as` saturates on overflow rather than wrapping, and the clamp keeps the
    // result inside the range movement's `i8` arithmetic assumes.
    (steps as i8).clamp(*HEIGHT_RANGE.start(), *HEIGHT_RANGE.end())
}
