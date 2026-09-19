//! The screenshots of a posture every gate passed.
//!
//! Four renders a human called wrong while all ~70 rules read clean, plus the
//! two side-by-side sheets the numbers behind them came from. They are the
//! pipeline's only fixture of a perception failure, so a later task that
//! deletes one hears about it here rather than in a review.
//!
//! Nothing measures them yet. Task 08 of
//! `docs/tasks/2026_09_19_animation_quality/` is the first that will.

use std::path::{Path, PathBuf};

use crate::support::repo_root;

/// Where the four live, and the two renders beside them.
const DIR: &str = "crates/xtask-art/tests/fixtures/perception";

/// What a human saw that no rule did.
const PHOTOS: [&str; 4] = [
    "photo_1_run_back_arms_squished.png",
    "photo_2_idle_front_hands_backward.png",
    "photo_3_idle_side_chin_tucked.png",
    "photo_4_strafe_hunched.png",
];

/// The renders the research document's tables were read off.
const EVIDENCE: [&str; 2] = ["evidence_front_views.png", "evidence_side_views.png"];

/// One fixture, checked to be the image rather than a pointer to it. A
/// checkout without Git LFS leaves text here.
fn image(name: &str) -> PathBuf {
    let path = Path::new(DIR).join(name);
    let full = repo_root().join(&path);
    let bytes = std::fs::read(&full).unwrap_or_default();
    assert_eq!(
        bytes.get(..8),
        Some(b"\x89PNG\r\n\x1a\n".as_slice()),
        "{} is missing or is a Git LFS pointer, not a PNG. Run \
         `git lfs pull`, and in CI pass `lfs: true` to actions/checkout.",
        path.display()
    );
    full
}

#[test]
fn every_screenshot_of_the_posture_nothing_measured_is_still_here() {
    for name in PHOTOS {
        assert!(image(name).is_file(), "{name}");
    }
}

#[test]
fn the_renders_the_reported_numbers_were_read_off_are_still_here() {
    for name in EVIDENCE {
        assert!(image(name).is_file(), "{name}");
    }
}
