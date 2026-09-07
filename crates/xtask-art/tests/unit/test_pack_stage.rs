//! The pack stage and the previews, over synthetic baked frames.
//!
//! The bake itself needs Blender, but everything downstream of it reads loose
//! PNGs off disk, so writing those directly exercises the whole tail of the
//! pipeline, including the manifest the game loads.

use image::{Rgba, RgbaImage};
use xtask_art::pack::{self, CharacterAssets};
use xtask_art::preview;
use xtask_art::spec::Paths;
use xtask_art::stages;

use crate::stubs::a_godot_stub;
use crate::support::{EnvGuard, a_library, a_png, a_spec, install_skeleton};

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

/// A repository the pack stage can measure in: the skeleton is there because
/// `Rule::measured` takes a profile, not because any `atlas.*` limit reads
/// one. All three count defects and publish 0.
///
/// The stub Godot comes with it, because the stage hands the project to the
/// importer and no test may need Godot installed.
fn a_packed_repo() -> (tempfile::TempDir, EnvGuard) {
    let dir = tempfile::tempdir().unwrap();
    install_skeleton(dir.path());
    let mut env = EnvGuard::new();
    env.set(
        "MARROWFALL_GODOT_BIN",
        a_godot_stub(dir.path()).to_str().unwrap(),
    );
    (dir, env)
}

#[test]
fn pack_writes_one_atlas_per_animation_plus_the_manifest() {
    let library = a_library();
    let (dir, _env) = a_packed_repo();
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

/// Packing measures what it wrote, so the three `atlas.*` rules run at that
/// boundary and their report is beside every other stage's.
#[test]
fn packing_measures_the_atlases_it_just_wrote() {
    let library = a_library();
    let (dir, _env) = a_packed_repo();
    let paths = Paths::new(dir.path(), "survivor");
    write_animation(&paths.staging(), "idle", 3, 40);

    stages::pack(&a_spec("survivor"), &library, &paths).unwrap();

    let report = xtask_art::check::Report::read(
        &xtask_art::check::Artifacts::new(dir.path(), "pack", "survivor", 1)
            .unwrap()
            .report(),
    )
    .unwrap();
    let rules: std::collections::BTreeSet<&str> = report
        .findings()
        .iter()
        .map(|finding| finding.rule.as_str())
        .collect();
    assert_eq!(
        rules,
        [
            "atlas.frame_count",
            "atlas.manifest_schema",
            "atlas.trim_boxes"
        ]
        .into_iter()
        .collect()
    );
    assert!(!report.has_errors());
}

/// And a manifest the game could not draw stops that boundary rather than
/// being packed and shipped. Two rules fire on it: the game's own reader
/// refuses the file, and the cell count says how many rects are missing.
#[test]
fn a_manifest_claiming_a_frame_nobody_packed_stops_the_pack() {
    let library = a_library();
    let (dir, _env) = a_packed_repo();
    let paths = Paths::new(dir.path(), "survivor");
    let spec = a_spec("survivor");
    write_animation(&paths.staging(), "idle", 3, 40);
    stages::pack(&spec, &library, &paths).unwrap();

    let manifest = paths.assets().join("character.ron");
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(&manifest, text.replace("frames: 3,", "frames: 4,")).unwrap();

    let error = stages::check_atlases(&spec, &manifest, &paths)
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("2 defect(s) on atlas.frame_count, atlas.manifest_schema"),
        "got: {error}"
    );
}

#[test]
fn the_manifest_ends_with_a_newline() {
    let library = a_library();
    let (dir, _env) = a_packed_repo();
    let paths = Paths::new(dir.path(), "survivor");
    write_animation(&paths.staging(), "idle", 2, 40);
    stages::pack(&a_spec("survivor"), &library, &paths).unwrap();

    let text = std::fs::read_to_string(paths.assets().join("character.ron")).unwrap();
    assert!(
        text.ends_with('\n'),
        "otherwise the eof pre-commit hook trips"
    );
}

/// A seed is not a loadable sidecar, so the stage finishes the job by handing
/// the whole project to Godot's own importer.
#[test]
fn packing_hands_the_project_to_godots_own_importer() {
    let library = a_library();
    let (dir, mut env) = a_packed_repo();
    let argv = dir.path().join("godot.argv");
    env.set("MARROWFALL_STUB_ARGV", argv.to_str().unwrap());
    let paths = Paths::new(dir.path(), "survivor");
    write_animation(&paths.staging(), "idle", 2, 40);

    stages::pack(&a_spec("survivor"), &library, &paths).unwrap();

    let recorded = std::fs::read_to_string(&argv).expect("godot was never run");
    assert_eq!(
        recorded.lines().collect::<Vec<&str>>(),
        vec![
            "--headless",
            "--import",
            "--path",
            paths.godot_project().to_str().unwrap()
        ]
    );
}

/// And the file the importer wrote is the file that lands, because the two
/// lines the runtime needs are Godot's and not ours. Measured: without them
/// the game refuses the atlas with `can't load resource of class: 'Texture2D'`.
#[test]
fn the_sidecar_the_importer_wrote_is_what_lands() {
    let library = a_library();
    let (dir, _env) = a_packed_repo();
    let paths = Paths::new(dir.path(), "survivor");
    write_animation(&paths.staging(), "idle", 2, 40);

    stages::pack(&a_spec("survivor"), &library, &paths).unwrap();

    let text = std::fs::read_to_string(paths.assets().join("idle.png.import")).unwrap();
    assert!(text.contains("path.bptc="), "no imported path:\n{text}");
    assert!(text.contains("[deps]"), "no dependency block:\n{text}");
    // BC7, the whole reason a seed is written at all, and the import keeps it.
    assert!(text.contains("compress/mode=2"), "got:\n{text}");
}

/// A pack that cannot import ships atlases the game cannot load, so Godot's
/// own failure is the stage's, in Godot's own words.
#[test]
fn a_godot_that_cannot_import_stops_the_pack() {
    let library = a_library();
    let (dir, mut env) = a_packed_repo();
    let refuses = a_godot_that(dir.path(), "cannot-open", "ERROR: cannot open project", 1);
    env.set("MARROWFALL_GODOT_BIN", refuses.to_str().unwrap());
    let paths = Paths::new(dir.path(), "survivor");
    write_animation(&paths.staging(), "idle", 2, 40);

    let error = stages::pack(&a_spec("survivor"), &library, &paths)
        .unwrap_err()
        .to_string();

    assert!(error.contains("cannot open project"), "got: {error}");
}

/// Measured on 4.7.2: a file Godot fails to import still exits 0, and the
/// only sign is the line it printed on stderr. So the exit code cannot be the
/// gate, or a broken atlas would be packed and recorded as done.
#[test]
fn an_import_error_stops_the_pack_even_though_godot_exits_zero() {
    let library = a_library();
    let (dir, mut env) = a_packed_repo();
    let quiet = a_godot_that(
        dir.path(),
        "exits-zero",
        "ERROR: Error importing 'res://assets/characters/survivor/idle.png'.",
        0,
    );
    env.set("MARROWFALL_GODOT_BIN", quiet.to_str().unwrap());
    let paths = Paths::new(dir.path(), "survivor");
    write_animation(&paths.staging(), "idle", 2, 40);

    let error = stages::pack(&a_spec("survivor"), &library, &paths)
        .unwrap_err()
        .to_string();

    assert!(error.contains("Error importing"), "got: {error}");
}

/// And no Godot at all is named rather than skipped, for the same reason.
#[test]
fn a_missing_godot_stops_the_pack_by_name() {
    let library = a_library();
    let (dir, mut env) = a_packed_repo();
    env.set("MARROWFALL_GODOT_BIN", "definitely-not-installed-godot");
    let paths = Paths::new(dir.path(), "survivor");
    write_animation(&paths.staging(), "idle", 2, 40);

    let error = stages::pack(&a_spec("survivor"), &library, &paths)
        .unwrap_err()
        .to_string();

    assert!(error.contains("on PATH"), "got: {error}");
}

/// One Godot that prints a line on stderr and exits with a code a test chose.
fn a_godot_that(dir: &std::path::Path, name: &str, says: &str, code: u8) -> std::path::PathBuf {
    crate::stubs::an_executable(
        dir,
        &format!("godot-{name}.sh"),
        &format!("echo \"{says}\" >&2\nexit {code}\n"),
    )
}

/// Godot mints a `uid` whenever it cannot see one, and a resource id that
/// changes on every re-pack is a diff nobody asked for. So the seed carries
/// the one already on disk across.
#[test]
fn re_packing_keeps_the_resource_id_godot_already_assigned() {
    let library = a_library();
    let (dir, _env) = a_packed_repo();
    let paths = Paths::new(dir.path(), "survivor");
    write_animation(&paths.staging(), "idle", 2, 40);
    stages::pack(&a_spec("survivor"), &library, &paths).unwrap();

    let settings = paths.assets().join("idle.png.import");
    let imported = std::fs::read_to_string(&settings)
        .unwrap()
        .replace("uid://stubminted", "uid://abc123");
    std::fs::write(&settings, &imported).unwrap();

    stages::pack(&a_spec("survivor"), &library, &paths).unwrap();

    let after = std::fs::read_to_string(&settings).unwrap();
    assert!(
        after.contains("uid=\"uid://abc123\""),
        "the re-pack dropped the resource id Godot assigned:\n{after}"
    );
    assert_eq!(
        after, imported,
        "a re-pack of an unchanged atlas must leave the file alone"
    );
}

/// The seed itself, before Godot sees it: the first pack has no file to read a
/// `uid` from, so it must not leave a hole where one would go.
#[test]
fn a_seed_with_no_resource_id_yet_leaves_no_hole_for_one() {
    let dir = tempfile::tempdir().unwrap();
    let atlas = dir.path().join("idle.png");
    std::fs::write(&atlas, a_png()).unwrap();

    pack::seed_import_settings(&atlas).unwrap();

    let text = std::fs::read_to_string(dir.path().join("idle.png.import")).unwrap();
    assert!(!text.contains("uid="), "nothing had assigned one yet");
    // BC7, the whole reason these settings are written at all.
    assert!(text.contains("compress/mode=2"));
    assert!(
        !text.contains("\n\n\n"),
        "a blank run where the uid would go:\n{text}"
    );
}

#[test]
fn every_animation_of_a_character_is_packed_at_one_scale() {
    let library = a_library();
    let (dir, _env) = a_packed_repo();
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
    let (dir, _env) = a_packed_repo();
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
    let (dir, _env) = a_packed_repo();
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
    let (dir, _env) = a_packed_repo();
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
    let (dir, _env) = a_packed_repo();
    let paths = Paths::new(dir.path(), "survivor");
    preview::concept(&paths).unwrap();
    assert!(!paths.preview().join("concept.png").exists());
}

#[test]
fn the_model_preview_renders_the_thumbnails_it_was_given() {
    let (dir, _env) = a_packed_repo();
    let paths = Paths::new(dir.path(), "survivor");

    preview::model(&paths, &[a_png(), a_png()]).unwrap();

    let sheet = image::open(paths.preview().join("model.png")).unwrap();
    assert_eq!(sheet.width(), 4);
}

#[test]
fn undecodable_thumbnails_are_skipped_rather_than_failing_the_stage() {
    let (dir, _env) = a_packed_repo();
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
    let (dir, _env) = a_packed_repo();
    let paths = Paths::new(dir.path(), "survivor");
    write_animation(&paths.staging(), "idle", 4, 40);
    stages::pack(&a_spec("survivor"), &library, &paths).unwrap();

    let full = paths.preview().join("sheet.png");
    assert!(full.exists(), "packing writes the contact sheet");
    let sheet = image::open(&full).unwrap();
    assert!(sheet.width() > 0 && sheet.height() > 0);
    // And the copy that is committed beside the atlas, no wider than the
    // published edge so a pull request can open it.
    let committed = image::open(paths.assets().join("sheet.png")).unwrap();
    assert!(
        committed.width().max(committed.height()) <= preview::SHEET_MAX_EDGE,
        "{}x{}",
        committed.width(),
        committed.height()
    );
    assert_eq!(
        (sheet.width(), sheet.height()),
        (committed.width(), committed.height()),
        "this sheet is already under the edge, so the two are the same size"
    );
}

/// The atlas is shelf-packed, not a grid, so the recorded rect is the only way
/// to find a frame in it. Cropping by row and column instead lands on whatever
/// happens to sit at those coordinates, which is usually nothing.
#[test]
fn the_sprite_preview_shows_a_character_in_every_direction() {
    let library = a_library();
    let (dir, _env) = a_packed_repo();
    let paths = Paths::new(dir.path(), "survivor");
    write_animation(&paths.staging(), "idle", 4, 40);
    stages::pack(&a_spec("survivor"), &library, &paths).unwrap();

    let assets: CharacterAssets =
        ron::from_str(&std::fs::read_to_string(paths.assets().join("character.ron")).unwrap())
            .unwrap();
    let idle = &assets.animations["idle"];
    let sheet = image::open(paths.preview().join("sheet.png"))
        .unwrap()
        .to_rgba8();

    for (index, name) in idle.directions.iter().enumerate() {
        let panel = image::imageops::crop_imm(
            &sheet,
            index as u32 * idle.cell_width,
            0,
            idle.cell_width,
            idle.cell_height,
        )
        .to_image();
        let drawn = panel
            .pixels()
            .filter(|pixel| **pixel != preview::BACKDROP)
            .count();
        assert!(
            drawn > 0,
            "the {name} panel is bare backdrop, the crop missed the frame"
        );
    }
}

#[test]
fn a_missing_atlas_is_skipped_rather_than_failing_the_preview() {
    let library = a_library();
    let (dir, _env) = a_packed_repo();
    let paths = Paths::new(dir.path(), "survivor");
    write_animation(&paths.staging(), "idle", 2, 40);
    stages::pack(&a_spec("survivor"), &library, &paths).unwrap();

    // Delete the atlas the manifest points at, then re-render the preview.
    let assets: CharacterAssets =
        ron::from_str(&std::fs::read_to_string(paths.assets().join("character.ron")).unwrap())
            .unwrap();
    std::fs::remove_file(paths.assets().join("idle.png")).unwrap();
    std::fs::remove_file(paths.preview().join("sheet.png")).unwrap();

    preview::sheet(&assets, &paths).unwrap();
    assert!(
        !paths.preview().join("sheet.png").exists(),
        "nothing to draw means no sheet, not a failed stage"
    );
}

#[test]
fn the_bake_preview_shows_one_row_per_animation() {
    let (dir, _env) = a_packed_repo();
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
    let (dir, _env) = a_packed_repo();
    let paths = Paths::new(dir.path(), "survivor");
    std::fs::create_dir_all(paths.staging()).unwrap();

    preview::bake(&["idle"], pack::direction_names(8).unwrap(), &paths).unwrap();
    assert!(!paths.preview().join("bake.png").exists());
}
