//! What the player sees, as frames and one contact sheet per state.
//!
//! Windowed on purpose. Measured on 4.7.2: `--headless --write-movie` renders
//! nothing, aborts on a null parameter and leaves only the audio track, while
//! the same run with a window writes real frames. So this is a local verb
//! rather than a gate, and the headless smoke test is what CI runs.

use std::path::{Path, PathBuf};

use game::{Locomotion, Vec2};
use image::RgbaImage;
use render::draw::{self, Clip};
use sprites::AnimationAtlas;

use crate::{engine, fixture, sheet};

/// What asks for the harness. Off by default: it opens a window, which no
/// unattended run wants and no CI runner has.
const ASKED: &str = "MARROWFALL_VISUAL_HARNESS";

/// Every locomotion state the renderer maps a clip to.
const STATES: [Locomotion; 5] = [
    Locomotion::Idle,
    Locomotion::Forward,
    Locomotion::Backward,
    Locomotion::StrafeLeft,
    Locomotion::StrafeRight,
];

/// The two facings every state is drawn in, in tile space. `(1, 1)` is row 0,
/// the survivor toward the camera, and `(-1, -1)` is the row half a turn
/// round, away from it. A row map that mirrored itself would put the wrong one
/// of the pair on the sheet.
const FACINGS: [Vec2; 2] = [Vec2::new(1.0, 1.0), Vec2::new(-1.0, -1.0)];

#[test]
fn every_locomotion_state_draws_a_full_loop_in_both_facings() {
    if std::env::var_os(ASKED).is_none() {
        println!("skipped: the visual harness opens a window, so set {ASKED}=1 to ask for it");
        return;
    }
    let Some(godot) = engine::godot_or_skip() else {
        return;
    };
    let root = fixture::root();
    engine::build_extension(&root);
    let scratch = fixture::scratch("visual");
    let project = fixture::project_copy(&scratch);
    engine::import(&godot, &scratch.join("import.log"), &project).expect_clean();

    let assets = fixture::manifests()
        .into_iter()
        .find(|(_, assets)| assets.name == "survivor")
        .expect("the survivor's manifest, which is the one the harness poses")
        .1;

    // Emptied first, so nothing a previous run left is mistaken for what the
    // committed atlases draw as today.
    let preview = root.join("art/preview/e2e");
    let _ = std::fs::remove_dir_all(&preview);

    for state in STATES {
        let clip = Clip::for_locomotion(state);
        let atlas = assets
            .animations
            .get(clip.name())
            .unwrap_or_else(|| panic!("the survivor has no {} atlas", clip.name()));
        let mut sheet_cells: Vec<RgbaImage> = Vec::new();
        for aim in FACINGS {
            let row = draw::row_for_aim(atlas, aim).expect("a row for the facing");
            let facing = &atlas.directions[row];
            let name = format!("{}_{facing}", state_name(state));
            let cells = record(
                &godot,
                Paths {
                    scratch: &scratch,
                    project: &project,
                    preview: &preview,
                    name: &name,
                },
                Pose { clip, aim, row },
                atlas,
            );
            judge(&cells, &name);
            sheet_cells.extend(cells);
        }
        sheet::grid(
            &sheet_cells,
            atlas.frames,
            &preview.join(format!("{}.png", state_name(state))),
        );
    }
    println!("the sheets are under {}", preview.display());
}

/// One clip, one facing, and the row the two of them come out as.
struct Pose {
    clip: Clip,
    aim: Vec2,
    row: usize,
}

/// Where one recording works and where its frames land. `name` is the
/// `<state>_<facing>` both of them are keyed by.
struct Paths<'a> {
    scratch: &'a Path,
    project: &'a Path,
    preview: &'a Path,
    name: &'a str,
}

/// Records one loop, and writes each frame cropped to the character.
///
/// Deterministic: `--fixed-fps` at the clip's own rate, the harness drawing
/// atlas frame `n` on frame `n`, and `--quit-after` at the frame count the
/// manifest declares. Nothing here reads a clock.
fn record(godot: &str, at: Paths<'_>, pose: Pose, atlas: &AnimationAtlas) -> Vec<RgbaImage> {
    // Godot writes the numbered frames beside this path and will not create
    // the directory itself.
    let movie = at.scratch.join("movie").join(at.name);
    std::fs::create_dir_all(&movie).expect("creating the movie directory");

    let run = engine::run(
        godot,
        &at.scratch.join(format!("{}.log", at.name)),
        &[
            "--path",
            engine::text(at.project),
            "res://scenes/pose.tscn",
            "--write-movie",
            engine::text(&movie.join("frame.png")),
            "--fixed-fps",
            &atlas.fps.to_string(),
            "--quit-after",
            &atlas.frames.to_string(),
            "--",
            &format!("--clip={}", pose.clip.name()),
            &format!("--aim={},{}", pose.aim.x, pose.aim.y),
        ],
    );
    run.expect_clean();
    // The harness and this test map the facing to a row independently, so the
    // line is where the two readings meet.
    run.expect_printed(&format!(
        "[marrowfall] pose {} row {} {} at",
        pose.clip.name(),
        pose.row,
        atlas.directions[pose.row]
    ));

    let recorded = frames(&movie);
    assert_eq!(
        recorded.len(),
        atlas.frames as usize,
        "{} recorded {} frames of {}",
        movie.display(),
        recorded.len(),
        atlas.frames
    );
    let dest = at.preview.join(at.name);
    recorded
        .iter()
        .enumerate()
        .map(|(index, frame)| {
            let cropped = sheet::cell(frame, atlas);
            sheet::write(&cropped, &dest.join(format!("frame_{index:04}.png")));
            cropped
        })
        .collect()
}

/// Every frame Godot's movie writer left, in playback order.
fn frames(movie: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(movie)
        .expect("reading the movie directory")
        .map(|entry| entry.expect("a movie directory entry").path())
        .filter(|path| path.extension().is_some_and(|kind| kind == "png"))
        .collect();
    found.sort();
    found
}

/// The three ways a loop is broken without anything having errored.
fn judge(cells: &[RgbaImage], name: &str) {
    assert!(!cells.is_empty(), "{name} rendered nothing");
    for (index, cell) in cells.iter().enumerate() {
        // The movie writer records the backdrop too, so an empty frame is a
        // flat one rather than a transparent one.
        let first = cell.pixels().next().expect("a frame with pixels");
        assert!(
            cell.pixels().any(|pixel| pixel != first),
            "{name} frame {index} drew nothing but the backdrop"
        );
    }
    let moved = cells.windows(2).filter(|pair| pair[0] != pair[1]).count();
    assert_eq!(
        moved,
        cells.len() - 1,
        "{name} repeats a frame, so the loop sticks"
    );
}

/// What a state's frames land under. Exhaustive, so a sixth state cannot be
/// added without a name for its own frames.
fn state_name(state: Locomotion) -> &'static str {
    match state {
        Locomotion::Idle => "idle",
        Locomotion::Forward => "forward",
        Locomotion::Backward => "backward",
        Locomotion::StrafeLeft => "strafe_left",
        Locomotion::StrafeRight => "strafe_right",
    }
}
