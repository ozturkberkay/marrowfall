//! The pack stage and the previews, over synthetic baked frames.
//!
//! The bake itself needs Blender, but everything downstream of it reads loose
//! PNGs off disk — so writing those directly exercises the whole tail of the
//! pipeline, including the manifest the game loads.

use image::{Rgba, RgbaImage};
use xtask_art::pack::{self, CharacterAssets};
use xtask_art::preview;
use xtask_art::spec::Paths;
use xtask_art::stages;

use crate::support::{a_library, a_png, a_spec};

/// Writes one frame: an opaque block inset from the canvas edge, so cropping
/// has something to find and nothing touches the border.
fn write_frame(dir: &std::path::Path, name: &str, direction: &str, index: usize, height: u32) {
    let mut image = RgbaImage::new(64, 64);
    for y in 8..(8 + height).min(56) {
        for x in 24..40 {
            image.put_pixel(x, y, Rgba([200, 180, 160, 255]));
        }
    }
    std::fs::create_dir_all(dir).unwrap();
    image
        .save(dir.join(format!("{name}_{direction}_{index:02}.png")))
        .unwrap();
}

/// A full set of frames for one animation across all 8 directions.
fn write_animation(dir: &std::path::Path, name: &str, frames: usize, height: u32) {
    for direction in pack::direction_names(8).unwrap() {
        for index in 0..frames {
            write_frame(dir, name, direction, index, height);
        }
    }
}

#[test]
fn pack_writes_one_atlas_per_animation_plus_the_manifest() {
    let library = a_library();
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let mut spec = a_spec("survivor");
    spec.animations.push("run".to_owned());
    write_animation(&paths.staging(), "idle", 4, 40);
    write_animation(&paths.staging(), "run", 3, 40);

    let record = stages::pack(&spec, &library, &paths).unwrap();

    assert!(paths.assets().join("idle.png").exists());
    assert!(paths.assets().join("run.png").exists());
    let manifest = paths.assets().join("character.ron");
    let assets: CharacterAssets = ron::from_str(&std::fs::read_to_string(&manifest).unwrap())
        .expect("the manifest the game loads must parse");
    assert_eq!(assets.name, "survivor");
    assert_eq!(assets.animations.len(), 2);
    assert!(record.note.unwrap().contains("2 atlases"));
}

#[test]
fn the_manifest_ends_with_a_newline() {
    let library = a_library();
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    write_animation(&paths.staging(), "idle", 2, 40);
    stages::pack(&a_spec("survivor"), &library, &paths).unwrap();

    let text = std::fs::read_to_string(paths.assets().join("character.ron")).unwrap();
    assert!(
        text.ends_with('\n'),
        "otherwise the eof pre-commit hook trips"
    );
}

#[test]
fn every_animation_of_a_character_is_packed_at_one_scale() {
    let library = a_library();
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let mut spec = a_spec("survivor");
    spec.animations.push("run".to_owned());
    // The run reaches taller than the idle; the character must not shrink.
    write_animation(&paths.staging(), "idle", 2, 30);
    write_animation(&paths.staging(), "run", 2, 44);
    stages::pack(&spec, &library, &paths).unwrap();

    let assets: CharacterAssets =
        ron::from_str(&std::fs::read_to_string(paths.assets().join("character.ron")).unwrap())
            .unwrap();
    let idle = &assets.animations["idle"];
    let run = &assets.animations["run"];
    assert!(
        run.cell_height > idle.cell_height,
        "a taller pose must occupy more pixels, not be rescaled to fit"
    );
}

#[test]
fn packing_reports_which_animation_has_no_frames() {
    let library = a_library();
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    std::fs::create_dir_all(paths.staging()).unwrap();

    let error = stages::pack(&a_spec("survivor"), &library, &paths)
        .unwrap_err()
        .to_string();
    assert!(error.contains("idle"), "got: {error}");
}

#[test]
fn a_direction_ring_the_bake_cannot_produce_is_rejected() {
    let library = a_library();
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    let mut spec = a_spec("survivor");
    spec.bake.directions = 6;

    let error = stages::pack(&spec, &library, &paths)
        .unwrap_err()
        .to_string();
    assert!(error.contains('6'), "got: {error}");
}

// --- previews -------------------------------------------------------------

#[test]
fn the_concept_preview_lays_the_views_out_in_one_row() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    for view in xtask_art::spec::View::ALL {
        let path = paths.concept(view);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, a_png()).unwrap();
    }

    preview::concept(&paths).unwrap();

    let sheet = image::open(paths.preview().join("concept.png")).unwrap();
    assert_eq!(sheet.width(), 8, "four 2px views side by side");
    assert_eq!(sheet.height(), 2);
}

#[test]
fn no_concepts_on_disk_writes_no_preview_rather_than_failing() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    preview::concept(&paths).unwrap();
    assert!(!paths.preview().join("concept.png").exists());
}

#[test]
fn the_model_preview_renders_the_thumbnails_it_was_given() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");

    preview::model(&paths, &[a_png(), a_png()]).unwrap();

    let sheet = image::open(paths.preview().join("model.png")).unwrap();
    assert_eq!(sheet.width(), 4);
}

#[test]
fn undecodable_thumbnails_are_skipped_rather_than_failing_the_stage() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");

    preview::model(&paths, &[b"not a png".to_vec()]).unwrap();

    assert!(
        !paths.preview().join("model.png").exists(),
        "a preview is advisory; a bad thumbnail must not stop the pipeline"
    );
}

#[test]
fn the_sprite_preview_is_written_from_the_packed_atlases() {
    let library = a_library();
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    write_animation(&paths.staging(), "idle", 4, 40);
    stages::pack(&a_spec("survivor"), &library, &paths).unwrap();

    let sprites = paths.preview().join("sprites.png");
    assert!(sprites.exists(), "packing writes the review sheet");
    let sheet = image::open(&sprites).unwrap();
    assert!(sheet.width() > 0 && sheet.height() > 0);
}

#[test]
fn a_missing_atlas_is_skipped_rather_than_failing_the_preview() {
    let library = a_library();
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    write_animation(&paths.staging(), "idle", 2, 40);
    stages::pack(&a_spec("survivor"), &library, &paths).unwrap();

    // Delete the atlas the manifest points at, then re-render the preview.
    let assets: CharacterAssets =
        ron::from_str(&std::fs::read_to_string(paths.assets().join("character.ron")).unwrap())
            .unwrap();
    std::fs::remove_file(paths.assets().join("idle.png")).unwrap();
    std::fs::remove_file(paths.preview().join("sprites.png")).unwrap();

    preview::sprites(&assets, &paths).unwrap();
    assert!(
        !paths.preview().join("sprites.png").exists(),
        "nothing to draw means no sheet, not a failed stage"
    );
}

#[test]
fn the_bake_preview_shows_one_row_per_animation() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    write_animation(&paths.staging(), "idle", 3, 40);
    write_animation(&paths.staging(), "run", 3, 40);

    preview::bake(&["idle", "run"], pack::direction_names(8).unwrap(), &paths).unwrap();

    let sheet = image::open(paths.preview().join("bake.png")).unwrap();
    assert_eq!(sheet.width(), 64 * 8, "one column per direction");
    assert_eq!(sheet.height(), 64 * 2, "one row per animation");
}

#[test]
fn no_baked_frames_writes_no_preview_rather_than_failing() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path(), "survivor");
    std::fs::create_dir_all(paths.staging()).unwrap();

    preview::bake(&["idle"], pack::direction_names(8).unwrap(), &paths).unwrap();
    assert!(!paths.preview().join("bake.png").exists());
}
