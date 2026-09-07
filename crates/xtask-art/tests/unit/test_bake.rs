//! The four `bake.*` rules Rust reads off the rendered frames.
//!
//! The 1,472 real frames of the survivor are gitignored derived output, so
//! what is calibrated here is the measurement and every way it must fail. The
//! real readings are in `[profile.bake]` beside each limit, and the recorded
//! run in `crates/xtask-art/tests/fixtures/bake.survivor.1.json` is the report
//! those numbers came out of.

use std::collections::BTreeSet;
use std::path::Path;

use xtask_art::check::profile::Profile;
use xtask_art::check::{Finding, Severity, bake};

use crate::frames::{
    CANVAS, a_clipped_frame, a_frame_offset_by, a_rendered_set, an_empty_frame, frame_path,
    write_frame,
};
use crate::support::{a_ring, repo_root};

const CLIP: &str = "idle";
const FRAMES: u32 = 3;

fn profile() -> Profile {
    Profile::of(&repo_root(), xtask_art::library::HUMANOID).expect("the committed profile")
}

/// One clip's rendered set, whole.
fn a_baked_clip() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    a_rendered_set(dir.path(), CLIP, a_ring(), FRAMES);
    dir
}

fn measured(dir: &Path) -> Vec<Finding> {
    bake::check_files(
        &[bake::Rendered {
            name: CLIP,
            dir,
            directions: a_ring(),
        }],
        &profile(),
        1,
    )
}

fn one(findings: &[Finding], rule: &str) -> Finding {
    let found: Vec<&Finding> = findings.iter().filter(|f| f.rule == rule).collect();
    assert_eq!(found.len(), 1, "one finding under {rule}: {found:?}");
    found[0].clone()
}

/// Every rule reports on every subject it owns, whatever the frames turn out
/// to hold. Deleting one measurement leaves its rule unread, which is what
/// the bake stage then refuses.
#[test]
fn every_file_rule_reports_once_per_clip() {
    let dir = a_baked_clip();
    a_rendered_set(dir.path(), "run", a_ring(), FRAMES);
    let findings = bake::check_files(
        &[
            bake::Rendered {
                name: CLIP,
                dir: dir.path(),
                directions: a_ring(),
            },
            bake::Rendered {
                name: "run",
                dir: dir.path(),
                directions: a_ring(),
            },
        ],
        &profile(),
        1,
    );

    let seen: BTreeSet<(String, String)> = findings
        .iter()
        .map(|f| (f.rule.clone(), f.subject.clone()))
        .collect();
    let mut owed: BTreeSet<(String, String)> = BTreeSet::new();
    for rule in [
        "bake.frame_count",
        "bake.non_empty",
        "bake.in_frame",
        "bake.pivot",
    ] {
        for clip in [CLIP, "run"] {
            owed.insert((rule.to_owned(), clip.to_owned()));
        }
    }
    assert_eq!(seen, owed);
}

/// The whole synthetic set holds every rule, which is the calibration the
/// negatives below are read against.
#[test]
fn a_whole_rendered_set_holds_every_rule() {
    let dir = a_baked_clip();
    let findings = measured(dir.path());

    for finding in &findings {
        assert_eq!(finding.severity, Severity::Info, "{finding:?}");
    }
    assert_eq!(
        one(&findings, "bake.frame_count").message,
        "idle rendered 24 of 24 frames, 8 directions x 3 frames"
    );
    assert_eq!(
        one(&findings, "bake.pivot").message,
        format!(
            "the worst of 12 opposite pair(s) of idle is {} against {} at frame 00, 0 px off \
             its own reflection",
            a_ring()[0],
            a_ring()[4],
        )
    );
    assert_eq!(
        one(&findings, "bake.in_frame").message,
        format!(
            "the tightest frame of idle is {} 00, 12 px clear of the nearest border",
            a_ring()[0]
        )
    );
    // 16 by 40 opaque pixels of a 64 by 64 canvas.
    assert_eq!(one(&findings, "bake.non_empty").measured, 15.625);
    assert_eq!(
        one(&findings, "bake.non_empty").message,
        format!(
            "the emptiest frame of idle is {} 00, at 15.6250 percent of its canvas",
            a_ring()[0]
        )
    );
}

/// `[synth]` one frame deleted. The rendered set is a rectangle of one frame
/// per direction per frame, and one gap in it is one slot the packer would
/// find empty.
#[test]
fn a_deleted_frame_is_counted_and_named() {
    let dir = a_baked_clip();
    let gone = frame_path(dir.path(), CLIP, a_ring()[2], 1);
    std::fs::remove_file(&gone).unwrap();

    let finding = one(&measured(dir.path()), "bake.frame_count");

    assert_eq!(finding.severity, Severity::Error);
    assert_eq!(finding.measured, 1.0);
    assert_eq!(
        finding.message,
        format!(
            "idle rendered 23 of 24 frames, 8 directions x 3 frames, missing {} 01",
            a_ring()[2]
        )
    );
}

/// `[synth]` one frame fully transparent, which is what a bake that lost its
/// camera, its lights or its mesh leaves behind.
#[test]
fn a_frame_that_rendered_nothing_fails_the_coverage_floor() {
    let dir = a_baked_clip();
    write_frame(dir.path(), CLIP, a_ring()[1], 2, &an_empty_frame());

    let finding = one(&measured(dir.path()), "bake.non_empty");

    assert_eq!(finding.severity, Severity::Error);
    assert_eq!(finding.measured, 0.0);
    assert_eq!(finding.limit, 1.0);
    assert_eq!(
        finding.message,
        format!(
            "the emptiest frame of idle is {} 02, at 0.0000 percent of its canvas",
            a_ring()[1]
        )
    );
}

/// `[synth]` a pose clipped at the border. Packing cannot repair it: the
/// pixels are already missing.
#[test]
fn a_pose_against_the_border_fails_in_frame() {
    let dir = a_baked_clip();
    write_frame(dir.path(), CLIP, a_ring()[0], 0, &a_clipped_frame());

    let finding = one(&measured(dir.path()), "bake.in_frame");

    assert_eq!(finding.severity, Severity::Error);
    assert_eq!(finding.measured, 0.0);
    assert_eq!(finding.limit, 1.0);
    assert_eq!(
        finding.message,
        format!(
            "the tightest frame of idle is {} 00, 0 px clear of the nearest border",
            a_ring()[0]
        )
    );
}

/// `[synth]` a frame offset 20 px. Opposite directions are exact reflections
/// of one another about the canvas center, so a frame drawn about any other
/// pivot shows up on the pair.
#[test]
fn a_frame_offset_twenty_pixels_breaks_the_reflection() {
    let dir = a_baked_clip();
    write_frame(dir.path(), CLIP, a_ring()[0], 1, &a_frame_offset_by(20));

    let finding = one(&measured(dir.path()), "bake.pivot");

    assert_eq!(finding.severity, Severity::Error);
    assert_eq!(finding.measured, 20.0);
    assert_eq!(finding.limit, 2.0);
    assert_eq!(
        finding.message,
        format!(
            "the worst of 12 opposite pair(s) of idle is {} against {} at frame 01, 20 px off \
             its own reflection",
            a_ring()[0],
            a_ring()[4],
        )
    );
}

/// And one pixel of it, which is what rasterizing an antialiased edge leaves
/// and what the limit is set to absorb.
#[test]
fn a_pixel_of_reflection_error_still_holds() {
    let dir = a_baked_clip();
    write_frame(dir.path(), CLIP, a_ring()[0], 1, &a_frame_offset_by(1));

    let finding = one(&measured(dir.path()), "bake.pivot");

    assert_eq!(finding.severity, Severity::Info);
    assert_eq!(finding.measured, 1.0);
}

/// A clip that rendered nothing at all has no set to count, and no coverage,
/// no inset and no reflection either. Undefined, never a skip: a gate that
/// goes quiet on absent input proves nothing.
#[test]
fn a_clip_with_no_frames_is_undefined_under_every_rule() {
    let dir = tempfile::tempdir().unwrap();
    let findings = measured(dir.path());

    for rule in [
        "bake.frame_count",
        "bake.non_empty",
        "bake.in_frame",
        "bake.pivot",
    ] {
        let finding = one(&findings, rule);
        assert_eq!(finding.severity, Severity::Error, "{finding:?}");
        assert_eq!(finding.unit, "undefined measurements", "{finding:?}");
    }
    assert!(
        one(&findings, "bake.frame_count")
            .message
            .contains("left no frame at all"),
    );
}

/// A file that is not an image is the same answer, and the reason says which
/// file.
#[test]
fn an_unreadable_frame_is_undefined_and_names_the_file() {
    let dir = a_baked_clip();
    let broken = frame_path(dir.path(), CLIP, a_ring()[3], 0);
    std::fs::write(&broken, b"not a png").unwrap();

    let finding = one(&measured(dir.path()), "bake.non_empty");

    assert_eq!(finding.severity, Severity::Error);
    assert!(
        finding.message.contains("holds no readable image"),
        "got: {}",
        finding.message
    );
    assert!(
        finding
            .message
            .contains(&format!("{}_{}_00.png", CLIP, a_ring()[3])),
        "got: {}",
        finding.message
    );
}

/// A ring with no opposite facing has nothing to reflect about. Every other
/// rule still reports.
#[test]
fn a_ring_with_no_opposite_facing_cannot_be_reflected() {
    let dir = tempfile::tempdir().unwrap();
    let odd = ["s", "w", "n"];
    a_rendered_set(dir.path(), CLIP, &odd, FRAMES);

    let findings = bake::check_files(
        &[bake::Rendered {
            name: CLIP,
            dir: dir.path(),
            directions: &odd,
        }],
        &profile(),
        1,
    );

    let pivot = one(&findings, "bake.pivot");
    assert_eq!(pivot.severity, Severity::Error);
    assert_eq!(
        pivot.message,
        "a ring of 3 direction(s) has no opposite facing to reflect idle about"
    );
    assert_eq!(one(&findings, "bake.frame_count").measured, 0.0);
}

/// The two directions a golden is taken in: the one facing the camera, and
/// the one three quarters around, which is a quarter turn the other way. A
/// joint moved along the camera's own line of sight barely moves in the
/// first and moves fully in the second.
#[test]
fn a_golden_is_taken_facing_the_camera_and_a_quarter_turn_from_it() {
    for count in [4u32, 8, 16, 32] {
        let ring = xtask_art::pack::direction_names(count).unwrap();
        let pair = bake::golden_directions(ring).unwrap();
        assert_eq!(pair[0], ring[0], "the first stop of a {count} ring");
        assert_eq!(pair[1], ring[count as usize * 3 / 4], "{count}");
    }
    assert_eq!(bake::golden_directions(&["s", "n"]), None, "too short");
    assert_eq!(bake::golden(CLIP, "e"), "idle_e");
}

/// The canvas the frames above are built on, so a reader of the numbers in
/// this file can check them.
#[test]
fn the_synthetic_canvas_is_square_and_sixty_four_pixels() {
    assert_eq!((CANVAS, CANVAS), (64, 64));
}
