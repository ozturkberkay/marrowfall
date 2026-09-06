//! The concept retry loop: three attempts, a fresh set of views on every one
//! of them, one numbered report each, and the images left where a human can
//! look at them.
//!
//! The stage itself is stubbed here. No key is spent proving that a loop
//! loops, and the four committed views are the only images this repository
//! measures.

use std::path::{Path, PathBuf};

use xtask_art::check::Report;
use xtask_art::cli::concept_until_it_holds;
use xtask_art::lock::StageRecord;
use xtask_art::spec::{Paths, View};

use crate::support::{a_concept_view, a_png, a_spec, install_skeleton};

/// A repository with the skeleton every published limit comes from.
fn a_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temp dir");
    install_skeleton(dir.path());
    dir
}

/// A view that fails the gates: a flat 2x2 with no figure in it, so
/// `single_figure` reads zero and the three rules that need a silhouette have
/// nothing to read. `background_flat` holds on it, at 0 levels.
fn a_failing_view() -> Vec<u8> {
    a_png()
}

/// Runs the loop against a stub stage that writes `views[attempt - 1]` as all
/// four views, repeating the last entry once it runs out.
///
/// Answers what the loop said, and how many attempts it generated.
async fn attempts(root: &Path, views: &[Vec<u8>]) -> (anyhow::Result<StageRecord>, usize) {
    let spec = a_spec("survivor");
    let paths = Paths::new(root, "survivor");
    let mut generated = 0usize;
    let outcome = concept_until_it_holds(&spec, &paths, root, || {
        generated += 1;
        let png = &views[(generated - 1).min(views.len() - 1)];
        for view in View::ALL {
            let file = paths.concept(view);
            std::fs::create_dir_all(file.parent().expect("a view has a parent")).expect("mkdir");
            std::fs::write(file, png).expect("writing a stub view");
        }
        std::future::ready(Ok(StageRecord::default()))
    })
    .await;
    (outcome, generated)
}

/// Where one attempt's report belongs.
fn report(root: &Path, attempt: u32) -> PathBuf {
    root.join(format!(
        "art/staging/reports/concept.survivor.{attempt}.json"
    ))
}

/// Three attempts, all failing: three reports, named and numbered, and the
/// images still on disk for a human to look at.
#[tokio::test]
async fn three_failures_leave_three_numbered_reports_and_keep_the_images() {
    let dir = a_repo();
    let root = dir.path();
    let (outcome, generated) = attempts(root, &[a_failing_view()]).await;

    let error = outcome.unwrap_err().to_string();
    assert_eq!(
        error,
        "the concept views of survivor failed their gates on all 3 attempts, listed in \
         art/staging/reports/concept.survivor.1.json, \
         art/staging/reports/concept.survivor.2.json, \
         art/staging/reports/concept.survivor.3.json. The images are still on disk, so \
         nothing has to be regenerated to look at them"
    );
    for attempt in 1..=3 {
        let written = Report::read(&report(root, attempt)).expect("a written report");
        assert_eq!(written.attempt(), attempt);
        assert_eq!((written.stage(), written.item()), ("concept", "survivor"));
        assert!(
            written.has_errors(),
            "attempt {attempt} was supposed to fail"
        );
    }
    assert_eq!(generated, 3, "one fresh set of views per attempt");
    for view in View::ALL {
        assert!(
            Paths::new(root, "survivor").concept(view).exists(),
            "{view} was thrown away, so nobody can see what failed"
        );
    }
}

/// A pass on the second attempt proceeds, and the second attempt generated a
/// new set: reusing the images that just failed would spend nothing and
/// change nothing while reporting a retry.
#[tokio::test]
async fn a_pass_on_the_second_attempt_proceeds_and_that_attempt_generated_again() {
    let dir = a_repo();
    let root = dir.path();
    let (outcome, generated) = attempts(root, &[a_failing_view(), a_concept_view()]).await;

    outcome.expect("the second attempt holds every gate");
    assert_eq!(generated, 2, "attempt two generated rather than reused");
    assert!(Report::read(&report(root, 1)).unwrap().has_errors());
    assert!(!Report::read(&report(root, 2)).unwrap().has_errors());
    assert!(
        !report(root, 3).exists(),
        "a third attempt was paid for after the second passed"
    );
}

/// Attempt one regenerates whatever is on disk, passing or failing. A set a
/// previous run left failing is the case that matters: reusing it would spend
/// one of the three attempts re-measuring images already known to be bad.
#[tokio::test]
async fn the_first_attempt_regenerates_a_set_a_previous_run_left_failing() {
    let dir = a_repo();
    let root = dir.path();
    let paths = Paths::new(root, "survivor");
    for view in View::ALL {
        let file = paths.concept(view);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, a_failing_view()).unwrap();
    }

    let (outcome, generated) = attempts(root, &[a_concept_view()]).await;

    outcome.expect("the fresh set holds every gate");
    assert_eq!(
        generated, 1,
        "one attempt, and it did not reuse what failed"
    );
    assert!(!Report::read(&report(root, 1)).unwrap().has_errors());
}

/// And the same when what is on disk was already passing, because whether the
/// stage runs at all is the plan's decision: reaching here means the operator
/// asked for new views and confirmed the spend.
#[tokio::test]
async fn the_first_attempt_regenerates_views_that_are_already_passing() {
    let dir = a_repo();
    let root = dir.path();
    let paths = Paths::new(root, "survivor");
    for view in View::ALL {
        let file = paths.concept(view);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, a_concept_view()).unwrap();
    }

    let (outcome, generated) = attempts(root, &[a_concept_view()]).await;

    outcome.expect("views that were already passing pass again");
    assert_eq!(
        generated, 1,
        "one attempt, and it did not reuse what was there"
    );
    assert!(!report(root, 2).exists());
}
