//! The three `atlas.*` rules, on the packed atlases and their manifest.
//!
//! The positive is the committed survivor: three atlases and the manifest the
//! game loads today. Every negative is a manifest edited by hand, because the
//! packer cannot produce one of them.

use std::path::Path;

use sprites::{Anchor, AnimationAtlas, CharacterAssets, FrameRect};
use xtask_art::check::profile::Profile;
use xtask_art::check::{Finding, Severity, atlas};

use crate::support::repo_root;

const CLIPS: [&str; 3] = ["idle", "run", "walk_back"];

fn profile() -> Profile {
    Profile::of(&repo_root(), xtask_art::library::HUMANOID).expect("the committed profile")
}

/// Where the survivor's shipped atlases live, checked to be the pixels
/// rather than a pointer to them: a checkout without Git LFS leaves text
/// there, and every rule below would then measure a file that is not the art.
fn committed() -> std::path::PathBuf {
    let dir = repo_root().join("project/assets/characters/survivor");
    for animation in CLIPS {
        let atlas = dir.join(format!("{animation}.png"));
        let bytes = std::fs::read(&atlas).unwrap_or_default();
        assert_eq!(
            bytes.get(1..4),
            Some(b"PNG".as_slice()),
            "{} is missing or is a Git LFS pointer. Run `git lfs pull`, and \
             in CI pass `lfs: true` to actions/checkout.",
            atlas.display()
        );
    }
    dir
}

fn measured(manifest: &Path, dir: &Path, animations: &[&str]) -> Vec<Finding> {
    atlas::check_files(
        &atlas::Packed {
            manifest,
            dir,
            animations,
        },
        &profile(),
        1,
    )
}

fn one(findings: &[Finding], rule: &str, subject: &str) -> Finding {
    let found: Vec<&Finding> = findings
        .iter()
        .filter(|f| f.rule == rule && f.subject == subject)
        .collect();
    assert_eq!(found.len(), 1, "one {rule} on {subject}: {found:?}");
    found[0].clone()
}

/// The committed manifest, with one substring rewritten, beside the real
/// atlases it indexes.
fn an_edited_manifest(from: &str, to: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let text = std::fs::read_to_string(committed().join("character.ron")).unwrap();
    assert!(text.contains(from), "nothing to edit: {from}");
    std::fs::write(dir.path().join("character.ron"), text.replace(from, to)).unwrap();
    dir
}

/// The calibration: the three committed atlases and the manifest the game
/// reads, with nothing wrong with them.
#[test]
fn the_committed_atlases_hold_every_rule() {
    let manifest = committed().join("character.ron");
    let findings = measured(&manifest, &committed(), &CLIPS);

    // Every rule on every subject it owns. Deleting one measurement leaves
    // its rule unread, which is what the pack stage then refuses.
    let seen: std::collections::BTreeSet<(&str, &str)> = findings
        .iter()
        .map(|f| (f.rule.as_str(), f.subject.as_str()))
        .collect();
    let name = manifest.display().to_string();
    let mut owed: std::collections::BTreeSet<(&str, &str)> =
        [("atlas.manifest_schema", name.as_str())].into();
    for clip in CLIPS {
        owed.insert(("atlas.frame_count", clip));
        owed.insert(("atlas.trim_boxes", clip));
    }
    assert_eq!(seen, owed);

    for finding in &findings {
        assert_eq!(finding.severity, Severity::Info, "{finding:?}");
        assert_eq!(finding.measured, 0.0, "{finding:?}");
    }
    assert_eq!(
        one(&findings, "atlas.manifest_schema", &name).message,
        format!("{name} loads as 3 animation(s) through the reader the game uses")
    );
    assert_eq!(
        one(&findings, "atlas.frame_count", "idle").message,
        "idle claims 16 direction(s) x 15 frame(s) and carries 240 rect(s)"
    );
    assert_eq!(
        one(&findings, "atlas.trim_boxes", "idle").message,
        "idle indexes a 2164x2048 atlas with 240 rect(s) in 93x240 cells"
    );
}

/// `[synth]` a manifest claiming one extra frame. The renderer indexes
/// `direction * frames + frame`, so a missing rect does not leave a gap: it
/// slides every later direction onto the wrong row.
#[test]
fn a_manifest_claiming_one_extra_frame_is_refused() {
    let dir = an_edited_manifest("frames: 15,", "frames: 16,");

    let finding = one(
        &measured(&dir.path().join("character.ron"), &committed(), &CLIPS),
        "atlas.frame_count",
        "idle",
    );

    assert_eq!(finding.severity, Severity::Error);
    assert_eq!(finding.measured, 16.0, "one whole direction of cells");
    assert_eq!(
        finding.message,
        "idle claims 16 direction(s) x 16 frame(s) and carries 240 rect(s)"
    );
}

/// `[synth]` a box one pixel outside the atlas it indexes.
#[test]
fn a_rect_one_pixel_past_the_atlas_is_refused() {
    let atlas = image::open(committed().join("idle.png")).unwrap();
    let dir = an_edited_manifest(
        "                FrameRect(\n                    x: 1297,",
        &format!(
            "                FrameRect(\n                    x: {},",
            atlas.width() - 1
        ),
    );
    std::fs::copy(committed().join("idle.png"), dir.path().join("idle.png")).unwrap();

    let finding = one(
        &measured(&dir.path().join("character.ron"), dir.path(), &["idle"]),
        "atlas.trim_boxes",
        "idle",
    );

    assert_eq!(finding.severity, Severity::Error);
    assert_eq!(finding.measured, 1.0);
    assert!(
        finding.message.contains("outside it: rect 0"),
        "{finding:?}"
    );
}

/// And an anchor outside its own cell, which is the other coordinate the
/// game reads a rect against.
#[test]
fn an_anchor_outside_its_cell_is_refused() {
    let dir = an_edited_manifest(
        "                x: 47,\n                y: 239,",
        "                x: 47,\n                y: 240,",
    );

    let finding = one(
        &measured(&dir.path().join("character.ron"), &committed(), &CLIPS),
        "atlas.trim_boxes",
        "idle",
    );

    assert_eq!(finding.severity, Severity::Error);
    assert_eq!(finding.measured, 1.0);
    assert!(
        finding.message.contains("outside it: the anchor"),
        "{finding:?}"
    );
}

/// `[synth]` a missing field. The manifest is the `sprites` types the game
/// reads it with, so a field they cannot be filled from is a character Godot
/// refuses at load time.
#[test]
fn a_manifest_missing_a_field_is_refused_and_leaves_the_rest_undefined() {
    let dir = an_edited_manifest("            fps: 8,\n", "");
    let manifest = dir.path().join("character.ron");

    let findings = measured(&manifest, &committed(), &CLIPS);

    let schema = one(
        &findings,
        "atlas.manifest_schema",
        &manifest.display().to_string(),
    );
    assert_eq!(schema.severity, Severity::Error);
    assert_eq!(schema.measured, 1.0);
    assert!(
        schema
            .message
            .contains("is refused by the reader the game uses: not a sprite manifest"),
        "{schema:?}"
    );
    for clip in CLIPS {
        for rule in ["atlas.frame_count", "atlas.trim_boxes"] {
            let finding = one(&findings, rule, clip);
            assert_eq!(finding.severity, Severity::Error, "{finding:?}");
            assert_eq!(finding.unit, "undefined measurements", "{finding:?}");
        }
    }
}

/// A manifest that is not there at all says so, under the same rule.
#[test]
fn a_manifest_that_was_never_written_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = dir.path().join("character.ron");

    let findings = measured(&manifest, dir.path(), &["idle"]);

    let schema = one(
        &findings,
        "atlas.manifest_schema",
        &manifest.display().to_string(),
    );
    assert_eq!(schema.severity, Severity::Error);
    assert!(schema.message.contains("could not be read"), "{schema:?}");
}

/// An animation the spec lists and the manifest does not carry is an atlas
/// the game will ask for and not find.
#[test]
fn an_animation_the_manifest_left_out_is_undefined() {
    let manifest = committed().join("character.ron");

    let findings = measured(&manifest, &committed(), &["strafe_left"]);

    let finding = one(&findings, "atlas.frame_count", "strafe_left");
    assert_eq!(finding.severity, Severity::Error);
    assert_eq!(
        finding.message,
        "the manifest carries no animation named strafe_left"
    );
}

/// And an atlas file the manifest names but nothing wrote.
#[test]
fn an_atlas_image_that_is_not_there_is_undefined() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(
        committed().join("character.ron"),
        dir.path().join("character.ron"),
    )
    .unwrap();

    let findings = measured(&dir.path().join("character.ron"), dir.path(), &["idle"]);

    let finding = one(&findings, "atlas.trim_boxes", "idle");
    assert_eq!(finding.severity, Severity::Error);
    assert!(
        finding.message.contains("holds no readable atlas"),
        "{finding:?}"
    );
    assert_eq!(
        one(&findings, "atlas.frame_count", "idle").severity,
        Severity::Info,
        "the numbers are readable without the picture"
    );
}

/// A two-direction, two-frame character, serialized through the same types
/// the game reads: small enough to break one invariant at a time, which no
/// packer output is. Mirrors the table in `crates/sprites/tests/unit`.
fn a_small_manifest(edit: impl FnOnce(&mut AnimationAtlas)) -> String {
    let rect = |x, y| FrameRect {
        x,
        y,
        w: 4,
        h: 8,
        off_x: 1,
        off_y: 2,
    };
    let mut atlas = AnimationAtlas {
        file: "idle.png".to_owned(),
        directions: vec!["s".to_owned(), "e".to_owned()],
        frames: 2,
        fps: 8,
        loops: true,
        cell_width: 10,
        cell_height: 20,
        anchor: Anchor { x: 5, y: 19 },
        rects: vec![rect(0, 0), rect(6, 0), rect(0, 10), rect(6, 10)],
    };
    edit(&mut atlas);
    ron::to_string(&CharacterAssets {
        name: "dummy".to_owned(),
        animations: [("idle".to_owned(), atlas)].into(),
    })
    .expect("the manifest types serialize")
}

/// That manifest on disk beside the 10x20 atlas its rects fit inside.
fn a_small_atlas(manifest: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("character.ron"), manifest).unwrap();
    image::RgbaImage::new(10, 20)
        .save(dir.path().join("idle.png"))
        .unwrap();
    dir
}

/// The control for the table below: nothing wrong with it, so every case
/// there fails on the one thing it broke.
#[test]
fn the_small_manifest_the_table_breaks_holds_every_rule() {
    let dir = a_small_atlas(&a_small_manifest(|_| {}));

    let findings = measured(&dir.path().join("character.ron"), dir.path(), &["idle"]);

    assert_eq!(findings.len(), 3, "{findings:#?}");
    for finding in &findings {
        assert_eq!(finding.severity, Severity::Info, "{finding:?}");
    }
}

/// `[synth]` every refusal `sprites::parse` has, one case each. The game's
/// own reader is the contract, so a manifest it will not load has to be an
/// error here rather than at load time.
#[test]
fn every_refusal_the_games_reader_has_is_an_error_here() {
    let cases: [(&str, String); 6] = [
        ("frames", a_small_manifest(|atlas| atlas.frames = 0)),
        ("fps", a_small_manifest(|atlas| atlas.fps = 0)),
        (
            "directions",
            a_small_manifest(|atlas| atlas.directions.clear()),
        ),
        ("rects", a_small_manifest(|atlas| atlas.rects.truncate(3))),
        (
            "anchor",
            a_small_manifest(|atlas| atlas.anchor.y = atlas.cell_height),
        ),
        ("the shape of the file", "not a manifest at all".to_owned()),
    ];

    for (refusal, manifest) in cases {
        assert!(
            sprites::parse(&manifest).is_err(),
            "the {refusal} case is one the game's reader accepts"
        );
        let dir = a_small_atlas(&manifest);

        let findings = measured(&dir.path().join("character.ron"), dir.path(), &["idle"]);

        assert!(
            findings.iter().any(|f| f.severity == Severity::Error),
            "{refusal}: no atlas rule calls this an error: {findings:#?}"
        );
    }
}

/// Every subject the pack boundary owes a finding on, so a rule that goes
/// quiet on one of them is a failing stage.
#[test]
fn the_owed_subjects_are_the_manifest_and_every_animation() {
    let manifest = Path::new("project/assets/characters/survivor/character.ron");

    assert_eq!(
        atlas::subjects(manifest, &CLIPS),
        vec![
            "project/assets/characters/survivor/character.ron",
            "idle",
            "run",
            "walk_back"
        ]
    );
}
