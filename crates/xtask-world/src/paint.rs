//! Turning a generated region into pixels.
//!
//! Kept apart from the command line and the filesystem so it is all testable:
//! `render` takes a world and returns an image, and nothing here touches a path.
//!
//! This is a map, not a game view. One pixel is one tile, seen from straight
//! above, because the question it answers is "is the world shaped right" and an
//! isometric projection would only make that harder to read.

use image::{Rgb, RgbImage};
use worldgen::{IVec2, World, height_at, region_at, sites_near};

/// What to draw.
#[derive(Debug, Clone, Copy)]
pub struct Shot {
    /// Tile at the centre of the image.
    pub centre: IVec2,
    /// Half-width in tiles. The image is `2 * radius / step` pixels square.
    pub radius: i32,
    /// Tiles per pixel. Above 1 the view covers more ground and samples it.
    pub step: i32,
    /// Draw a marker on every point of interest.
    pub sites: bool,
}

impl Shot {
    /// Pixels along one edge.
    #[must_use]
    pub fn side(self) -> u32 {
        // i64 for the doubling: `radius` is user supplied, and `2 * i32::MAX`
        // overflows. Clamped rather than wrapped, so a huge request is refused by
        // the image crate's own size limit instead of silently becoming small.
        let side = 2 * i64::from(self.radius) / i64::from(self.step);
        side.clamp(1, i64::from(u32::MAX)) as u32
    }
}

/// One hue per difficulty tier, so the gradient outward is the first thing the
/// eye finds. Green through to bone white, which is roughly the game's own
/// progression from dead grass to the frontier.
const TIER_HUES: [(u8, u8, u8); 6] = [
    (74, 96, 58),
    (66, 84, 74),
    (96, 84, 58),
    (104, 66, 50),
    (86, 52, 56),
    (120, 116, 112),
];

/// One colour per site class, in table order. Bright and unnatural on purpose:
/// a marker is an overlay, and it has to survive being drawn over any terrain.
const CLASS_MARKS: [(u8, u8, u8); 6] = [
    (255, 238, 120),
    (120, 204, 255),
    (255, 138, 202),
    (158, 255, 148),
    (255, 168, 88),
    (236, 236, 255),
];

/// Half-width of a marker, in pixels. A marker is a fixed size on screen rather
/// than in tiles, so it stays visible at any step.
const MARK_ARM: i32 = 2;

/// Draws the region around `shot`.
#[must_use]
pub fn render(world: &World, shot: Shot) -> RgbImage {
    let mut image = ground(world, shot);
    if shot.sites {
        mark_sites(world, shot, &mut image);
    }
    image
}

/// The terrain, with no markers over it.
fn ground(world: &World, shot: Shot) -> RgbImage {
    let side = shot.side();
    RgbImage::from_fn(side, side, |px, py| {
        Rgb(colour_of(world, tile_of(shot, px as i32, py as i32)))
    })
}

/// The tile one pixel samples.
fn tile_of(shot: Shot, px: i32, py: i32) -> IVec2 {
    IVec2::new(
        shot.centre.x - shot.radius + px * shot.step,
        shot.centre.y - shot.radius + py * shot.step,
    )
}

/// Stamps a cross on every site inside the view.
fn mark_sites(world: &World, shot: Shot, image: &mut RgbImage) {
    for site in sites_near(world, shot.centre, shot.radius) {
        let colour = CLASS_MARKS[usize::from(site.class.0) % CLASS_MARKS.len()];
        // The inverse of `tile_of`.
        let px = (site.at.x - shot.centre.x + shot.radius).div_euclid(shot.step);
        let py = (site.at.y - shot.centre.y + shot.radius).div_euclid(shot.step);
        // A cross rather than a dot, so one pixel of terrain showing through does
        // not hide the marker.
        for arm in -MARK_ARM..=MARK_ARM {
            plot(image, px + arm, py, colour);
            plot(image, px, py + arm, colour);
        }
    }
}

/// Sets one pixel, ignoring anything off the edge.
fn plot(image: &mut RgbImage, x: i32, y: i32, colour: (u8, u8, u8)) {
    if x < 0 || y < 0 {
        return;
    }
    let (x, y) = (x.unsigned_abs(), y.unsigned_abs());
    if x >= image.width() || y >= image.height() {
        return;
    }
    image.put_pixel(x, y, Rgb([colour.0, colour.1, colour.2]));
}

/// The colour of one tile: hue from its tier, a nudge from its biome so two
/// biomes in one tier are still distinguishable, and brightness from its height
/// so terraces read as relief.
fn colour_of(world: &World, tile: IVec2) -> [u8; 3] {
    let region = region_at(world, tile);
    let base = TIER_HUES[usize::from(region.tier).min(TIER_HUES.len() - 1)];

    // A per-biome offset, so two biomes sharing a tier are still told apart.
    // Large enough to read against the relief shading below, which otherwise
    // swamps it.
    let nudge = i32::from(region.biome.0 % 3) * 30 - 30;

    // Height shades the tile, so terraces read as relief. Deliberately gentler
    // than the biome offset: at a coarse step one pixel spans a whole terrace,
    // and a strong relief term turns that into speckle.
    let relief = i32::from(height_at(world, tile)) * 4;

    [
        clamp(i32::from(base.0) + nudge + relief),
        clamp(i32::from(base.1) + nudge + relief),
        clamp(i32::from(base.2) + nudge + relief),
    ]
}

fn clamp(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}
