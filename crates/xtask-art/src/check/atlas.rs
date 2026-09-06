//! The pack gates: what a packed atlas and its manifest must be before the
//! game is asked to draw them.
//!
//! The manifest format is `sprites`, the crate the game reads it with, so
//! nothing here describes that file a second time: [`MANIFEST_SCHEMA`] runs
//! `sprites::parse` itself, which is every invariant the game holds a
//! manifest to. The other two report the numbers behind two of those
//! invariants, and the one thing no format check can see: whether a rect is
//! inside the atlas image it indexes.

use std::path::Path;

use super::profile::Profile;
use super::{Comparison, Finding, Rule};
use crate::pack::{AnimationAtlas, CharacterAssets};

const SHAPE: &str = "the manifest, through `sprites::parse`, the reader the game loads it with";
const CELLS: &str = "the rects one animation carries, against the one per direction per frame its \
                     own header claims";
const BOXES: &str = "each rect of one animation and its anchor, against the atlas image it \
                     indexes and against the cell its own offset puts it in";

/// Whether `sprites::parse` accepts the manifest.
///
/// Anything the game's own reader refuses is a character it will not draw at
/// load time, which is the one failure the rest of this pipeline cannot see.
pub const MANIFEST_SCHEMA: Rule = Rule {
    id: "atlas.manifest_schema",
    comparison: Comparison::Eq,
    unit: "unreadable manifests",
    space: SHAPE,
    limit: |_| 0.0,
};

/// Whether every frame the manifest claims has a rect behind it.
///
/// The renderer indexes `direction * frames + frame`, so one missing rect
/// does not leave a gap: it slides every later direction onto the wrong row.
pub const FRAME_COUNT: Rule = Rule {
    id: "atlas.frame_count",
    comparison: Comparison::Eq,
    unit: "cells",
    space: CELLS,
    limit: |_| 0.0,
};

/// Whether every rect is inside the picture it indexes and inside its cell,
/// and whether the anchor is inside the cell too.
///
/// A box over the edge of the atlas samples whatever the sampler decides at
/// the edge, and one over the edge of its cell draws outside the sprite the
/// anchor was measured for.
pub const TRIM_BOXES: Rule = Rule {
    id: "atlas.trim_boxes",
    comparison: Comparison::Eq,
    unit: "rects",
    space: BOXES,
    limit: |_| 0.0,
};

/// Every atlas rule, in the order `--list-rules` prints them.
pub const RULES: [&Rule; 3] = [&MANIFEST_SCHEMA, &FRAME_COUNT, &TRIM_BOXES];

/// One character's packed assets: the manifest, the directory its atlases sit
/// in, and the animations the spec expects to find in it.
pub struct Packed<'a> {
    pub manifest: &'a Path,
    pub dir: &'a Path,
    /// The names the spec lists, so an animation the manifest simply left out
    /// is reported rather than passed over.
    pub animations: &'a [&'a str],
}

/// Runs every rule this module owns over one character's packed assets.
///
/// A file nothing can deserialize leaves the other two rules undefined on
/// every animation, never quiet: their numbers cannot be read at all until
/// something can fill the types that hold them.
pub fn check_files(packed: &Packed<'_>, profile: &Profile, attempt: u32) -> Vec<Finding> {
    let name = packed.manifest.display().to_string();
    let text = std::fs::read_to_string(packed.manifest)
        .map_err(|error| format!("{name} could not be read: {error}"));
    let loaded = text.as_ref().map_err(String::clone).and_then(|text| {
        sprites::parse(text)
            .map_err(|error| format!("{name} is refused by the reader the game uses: {error}"))
    });
    // RON alone for the numbers: an invariant can break and still leave them
    // readable, and then the number is what says how badly.
    let values = text.and_then(|text| {
        ron::from_str::<CharacterAssets>(&text)
            .map_err(|error| format!("{name} is not a manifest: {error}"))
    });

    let mut findings = vec![match &loaded {
        Ok(assets) => MANIFEST_SCHEMA.measured(
            profile,
            &name,
            0.0,
            attempt,
            format!(
                "{name} loads as {} animation(s) through the reader the game uses",
                assets.animations.len()
            ),
        ),
        Err(reason) => MANIFEST_SCHEMA.measured(profile, &name, 1.0, attempt, reason.clone()),
    }];
    match &values {
        Ok(assets) => {
            for animation in packed.animations {
                findings.extend(one_animation(
                    assets, animation, packed.dir, profile, attempt,
                ));
            }
        }
        Err(reason) => {
            for animation in packed.animations {
                findings.push(FRAME_COUNT.undefined(animation, attempt, reason.clone()));
                findings.push(TRIM_BOXES.undefined(animation, attempt, reason.clone()));
            }
        }
    }
    findings
}

/// Every subject a packed character owes a finding on: the manifest itself,
/// and each animation the spec named.
pub fn subjects(manifest: &Path, animations: &[&str]) -> Vec<String> {
    let mut owed = vec![manifest.display().to_string()];
    owed.extend(animations.iter().map(|name| (*name).to_owned()));
    owed
}

fn one_animation(
    assets: &CharacterAssets,
    animation: &str,
    dir: &Path,
    profile: &Profile,
    attempt: u32,
) -> Vec<Finding> {
    let Some(atlas) = assets.animations.get(animation) else {
        let reason = format!("the manifest carries no animation named {animation}");
        return vec![
            FRAME_COUNT.undefined(animation, attempt, reason.clone()),
            TRIM_BOXES.undefined(animation, attempt, reason),
        ];
    };
    let claimed = atlas.directions.len() * atlas.frames as usize;
    vec![
        FRAME_COUNT.measured(
            profile,
            animation,
            (claimed as f64 - atlas.rects.len() as f64).abs(),
            attempt,
            format!(
                "{animation} claims {} direction(s) x {} frame(s) and carries {} rect(s)",
                atlas.directions.len(),
                atlas.frames,
                atlas.rects.len()
            ),
        ),
        trim_boxes(atlas, animation, dir, profile, attempt),
    ]
}

fn trim_boxes(
    atlas: &AnimationAtlas,
    animation: &str,
    dir: &Path,
    profile: &Profile,
    attempt: u32,
) -> Finding {
    let file = dir.join(&atlas.file);
    let size = match image::image_dimensions(&file) {
        Ok(size) => size,
        Err(error) => {
            return TRIM_BOXES.undefined(
                animation,
                attempt,
                format!("{} holds no readable atlas: {error}", file.display()),
            );
        }
    };
    let (width, height) = size;
    let mut outside: Vec<String> = atlas
        .rects
        .iter()
        .enumerate()
        .filter_map(|(index, rect)| {
            let over_atlas = rect.x + rect.w > width || rect.y + rect.h > height;
            let over_cell =
                rect.off_x + rect.w > atlas.cell_width || rect.off_y + rect.h > atlas.cell_height;
            let empty = rect.w == 0 || rect.h == 0;
            (over_atlas || over_cell || empty).then(|| format!("rect {index}"))
        })
        .collect();
    if atlas.anchor.x >= atlas.cell_width || atlas.anchor.y >= atlas.cell_height {
        outside.push("the anchor".to_owned());
    }
    TRIM_BOXES.measured(
        profile,
        animation,
        outside.len() as f64,
        attempt,
        format!(
            "{animation} indexes a {width}x{height} atlas with {} rect(s) in {}x{} cells{}",
            atlas.rects.len(),
            atlas.cell_width,
            atlas.cell_height,
            if outside.is_empty() {
                String::new()
            } else {
                format!(", outside it: {}", outside.join(", "))
            }
        ),
    )
}
