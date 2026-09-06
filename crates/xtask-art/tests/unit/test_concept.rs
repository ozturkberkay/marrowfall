//! The five concept gates, on the four committed views and on a negative
//! built from each of them.
//!
//! Every limit here was calibrated on those four views, and every negative is
//! the committed art with pixels moved: nothing calls a measurement to decide
//! what to break, so no fixture can agree with the gate it has to fail.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use xtask_art::check::concept::{self, Rendered, Silhouette};
use xtask_art::check::profile::Profile;
use xtask_art::check::{Finding, Severity, Symmetry};
use xtask_art::library::HUMANOID;
use xtask_art::spec::View;

use crate::concepts::Concept;
use crate::support::repo_root;

fn profile() -> Profile {
    Profile::of(&repo_root(), HUMANOID).expect("the committed humanoid profile")
}

/// The four committed views, which is the only concept art this repository
/// holds.
fn committed() -> PathBuf {
    repo_root().join("art/characters/survivor/concept")
}

/// Every view named the way the pipeline names it, out of one directory.
fn views(dir: &Path) -> Vec<(View, PathBuf)> {
    View::ALL
        .into_iter()
        .map(|view| (view, dir.join(format!("{view}.png"))))
        .collect()
}

fn rendered(files: &[(View, PathBuf)]) -> Vec<Rendered<'_>> {
    files
        .iter()
        .map(|(view, file)| Rendered {
            name: view.as_str(),
            file,
            torso_faces_the_camera: view.shows_the_torso(),
        })
        .collect()
}

/// Every finding on one directory of views, at the symmetry a caller declares.
fn measured(dir: &Path, symmetry: Symmetry) -> Vec<Finding> {
    let files = views(dir);
    concept::check_files(&rendered(&files), &profile(), symmetry, 1)
}

/// And the same on the committed art, which every limit was set from.
fn on_the_committed_views() -> Vec<Finding> {
    measured(&committed(), Symmetry::Enforced)
}

/// One rule's readings, by subject.
fn readings(findings: &[Finding], rule: &str) -> BTreeMap<String, f64> {
    findings
        .iter()
        .filter(|finding| finding.rule == rule)
        .map(|finding| (finding.subject.clone(), finding.measured))
        .collect()
}

fn errors(findings: &[Finding]) -> Vec<&Finding> {
    findings
        .iter()
        .filter(|finding| finding.severity == Severity::Error)
        .collect()
}

/// A directory holding the four committed views, one of them replaced by a
/// negative. Returns the directory, which must outlive the readings.
fn with_one_view_replaced(name: &str, broken: &Concept) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temp dir");
    for view in View::ALL {
        let file = dir.path().join(format!("{view}.png"));
        if view.as_str() == name {
            broken.write(&file);
        } else {
            std::fs::copy(committed().join(format!("{view}.png")), &file).expect("copying a view");
        }
    }
    dir
}

// --- what the four committed views read -----------------------------------

/// Every published limit is what these four views measured plus the headroom
/// the profile writes beside it. No number here is a guess, and this is the
/// test that says so.
#[test]
fn the_published_limits_are_the_ones_this_task_calibrated() {
    let profile = profile();
    let findings = on_the_committed_views();
    let round = |value: f64| (value * 1000.0).round() / 1000.0;
    let read = |rule: &str| -> BTreeMap<String, f64> {
        readings(&findings, rule)
            .into_iter()
            .map(|(subject, value)| (subject, round(value)))
            .collect()
    };

    assert_eq!(
        read("concept.background_flat"),
        BTreeMap::from([
            ("front".to_owned(), 3.0),
            ("back".to_owned(), 6.0),
            ("left".to_owned(), 3.0),
            ("right".to_owned(), 6.0),
        ]),
        "background_spread_levels is 2x the worst of these"
    );
    assert_eq!(profile.concept.background_spread_levels, 12.0);

    assert_eq!(
        read("concept.single_figure"),
        BTreeMap::from([
            ("front".to_owned(), 1.0),
            ("back".to_owned(), 1.0),
            ("left".to_owned(), 1.0),
            ("right".to_owned(), 1.0),
        ]),
        "one figure per view, which is the whole limit"
    );

    assert_eq!(
        read("concept.arm_gap"),
        BTreeMap::from([("front".to_owned(), 86.054), ("back".to_owned(), 86.316)]),
        "arm_gap_rows_percent sits 11 points under the worst of these"
    );
    assert_eq!(profile.concept.arm_gap_rows_percent, 75.0);

    assert_eq!(
        read("concept.mirror"),
        BTreeMap::from([("front".to_owned(), 1.167), ("back".to_owned(), 1.062)]),
        "mirror_percent is 2x the worst of these"
    );
    assert_eq!(profile.concept.mirror_percent, 2.4);

    assert_eq!(
        read("concept.cross_view"),
        BTreeMap::from([
            ("front against back".to_owned(), 3.256),
            ("front against left".to_owned(), 2.136),
            ("front against right".to_owned(), 2.624),
            ("back against left".to_owned(), 1.120),
            ("back against right".to_owned(), 0.757),
            ("left against right".to_owned(), 1.134),
        ]),
        "cross_view_percent sits 1.8x over the worst of these"
    );
    assert_eq!(profile.concept.cross_view_percent, 6.0);

    assert!(errors(&findings).is_empty(), "{:#?}", errors(&findings));
}

/// Every rule reports on every subject it owns, whether or not the reading
/// holds. Deleting any one measurement fails here rather than passing
/// quietly.
#[test]
fn every_rule_reports_on_every_view_it_owns() {
    let findings = on_the_committed_views();
    let subjects =
        |rule: &str| -> BTreeSet<String> { readings(&findings, rule).into_keys().collect() };
    let four = BTreeSet::from([
        "front".to_owned(),
        "back".to_owned(),
        "left".to_owned(),
        "right".to_owned(),
    ]);
    let facing = BTreeSet::from(["front".to_owned(), "back".to_owned()]);

    assert_eq!(subjects("concept.background_flat"), four);
    assert_eq!(subjects("concept.single_figure"), four);
    // A side view shows the arms in front of the torso, so neither the gap
    // nor the reflection resolves a subject there.
    assert_eq!(subjects("concept.arm_gap"), facing);
    assert_eq!(subjects("concept.mirror"), facing);
    assert_eq!(subjects("concept.cross_view").len(), 6, "every pair");
    assert_eq!(findings.len(), 18, "4 + 4 + 2 + 2 + 6");
}

/// And the subject list the runner refuses an incomplete report against is
/// the same one.
#[test]
fn the_owed_subjects_are_every_view_and_every_pair_of_them() {
    let files = views(&committed());
    let owed = concept::subjects(&rendered(&files));

    assert_eq!(owed.len(), 10, "four views and their six pairs: {owed:?}");
    assert!(owed.contains(&"front".to_owned()), "{owed:?}");
    assert!(owed.contains(&"left against right".to_owned()), "{owed:?}");
    for finding in on_the_committed_views() {
        assert!(
            owed.contains(&finding.subject),
            "{} is not owed",
            finding.subject
        );
    }
}

// --- one negative per rule ------------------------------------------------

/// The reconstructor segments the figure off the backdrop, so a ramp in it is
/// a ramp in the outline of the mesh.
#[test]
fn a_gradient_in_the_background_fails_the_flatness_gate() {
    let broken = Concept::view("front").with_a_gradient(30.0);
    let dir = with_one_view_replaced("front", &broken);
    let findings = measured(dir.path(), Symmetry::Enforced);

    assert_eq!(
        readings(&findings, "concept.background_flat")["front"],
        24.0
    );
    let named: Vec<&str> = errors(&findings)
        .iter()
        .map(|finding| finding.rule.as_str())
        .collect();
    assert!(named.contains(&"concept.background_flat"), "{named:?}");
}

/// A second character in the frame is reconstructed into the same mesh as the
/// first.
#[test]
fn a_second_figure_in_the_frame_fails() {
    let broken = Concept::view("front").beside_a_second_figure();
    let dir = with_one_view_replaced("front", &broken);
    let findings = measured(dir.path(), Symmetry::Enforced);

    assert_eq!(readings(&findings, "concept.single_figure")["front"], 2.0);
    let failed = errors(&findings)
        .iter()
        .find(|finding| finding.rule == "concept.single_figure")
        .map(|finding| finding.message.clone())
        .unwrap_or_default();
    assert!(
        failed.contains("2 figure(s) of at least 786 pixels"),
        "got: {failed}"
    );
}

/// `concept.background_flat` histograms the pixels the border fill reached
/// and nothing else, so a wall over a floor is flat by construction: the fill
/// stops at the seam and never sees the second tone.
///
/// What the reading owes is the share it covers, and what catches the case is
/// `concept.single_figure`, because the unfilled tone is a piece of
/// silhouette of its own.
#[test]
fn a_second_backdrop_tone_reads_flat_and_fails_the_figure_count() {
    let broken = Concept::view("front").with_a_patch_on_the_wall();
    let dir = with_one_view_replaced("front", &broken);
    let findings = measured(dir.path(), Symmetry::Enforced);
    let said = |rule: &str| {
        findings
            .iter()
            .find(|finding| finding.rule == rule && finding.subject == "front")
            .map(|finding| finding.message.clone())
            .unwrap_or_default()
    };

    assert_eq!(
        readings(&findings, "concept.background_flat")["front"],
        3.0,
        "the fill never reached the second tone, so the spread is unchanged"
    );
    // 76.394 on the untouched view, and the patch is a fifth by a fifth of
    // the frame, so 3.982 percent of it went unread.
    assert_eq!(
        said("concept.background_flat"),
        "the background of front spans 3 levels over the 72.412 percent of the image the \
         border fill reaches"
    );
    assert_eq!(readings(&findings, "concept.single_figure")["front"], 2.0);
    assert!(
        errors(&findings)
            .iter()
            .any(|finding| finding.rule == "concept.single_figure"),
        "{findings:#?}"
    );
}

/// And the same fill in the other direction: a figure region the color of the
/// backdrop, touching it, is eaten and leaves what it held as an extra piece.
/// A white shirt on a white wall is the case, and this is a hood.
#[test]
fn a_figure_the_color_of_the_backdrop_is_eaten_and_shows_as_an_extra_figure() {
    let broken = Concept::view("front").with_the_neck_the_color_of_the_wall();
    let dir = with_one_view_replaced("front", &broken);
    let findings = measured(dir.path(), Symmetry::Enforced);

    assert_eq!(
        readings(&findings, "concept.background_flat")["front"],
        3.0,
        "the hood is the wall's own color, so it widens no spread"
    );
    assert_eq!(
        readings(&findings, "concept.single_figure")["front"],
        2.0,
        "the head is a piece of its own now"
    );
    assert!(
        errors(&findings)
            .iter()
            .any(|finding| finding.rule == "concept.single_figure"),
        "{findings:#?}"
    );
}

/// An arm painted onto the ribcage is reconstructed as part of it, and no
/// amount of rigging separates them afterwards.
#[test]
fn arms_painted_onto_the_ribcage_fail() {
    let broken = Concept::view("front").with_the_arms_on_the_ribcage();
    let dir = with_one_view_replaced("front", &broken);
    let findings = measured(dir.path(), Symmetry::Enforced);

    assert_eq!(readings(&findings, "concept.arm_gap")["front"], 0.0);
    assert!(
        errors(&findings)
            .iter()
            .any(|finding| finding.rule == "concept.arm_gap"),
        "the gaps are gone and the gate held anyway"
    );
    // The back view is untouched, so the rule still reports the reading the
    // limit was set from.
    assert_eq!(
        (readings(&findings, "concept.arm_gap")["back"] * 1000.0).round() / 1000.0,
        86.316
    );
}

/// The prompt asks for a symmetrical A-pose, and one arm longer than the
/// other is a mesh with one arm longer than the other.
#[test]
fn a_left_half_stretched_15_percent_fails_the_mirror() {
    let broken = Concept::view("front").with_the_left_half_stretched(1.15);
    let dir = with_one_view_replaced("front", &broken);
    let findings = measured(dir.path(), Symmetry::Enforced);

    let read = (readings(&findings, "concept.mirror")["front"] * 1000.0).round() / 1000.0;
    assert_eq!(read, 6.394, "against a limit of 2.4 and a committed 1.167");
    assert!(
        errors(&findings)
            .iter()
            .any(|finding| finding.rule == "concept.mirror"),
        "{findings:#?}"
    );
}

/// The three statistics the reflection carries, on both views the rule owns
/// and on the negative, which is the whole of why the published one is the
/// percentile.
///
/// The outright worst row is hair: 4.669 against the negative's 6.522 is
/// 1.4x, too close to set a limit between. The mean dilutes a defect that
/// covers a fifth of the rows: 0.301 against 1.260 is 4.2x. The percentile
/// drops the hair and keeps the defect: 1.167 against 6.394 is 5.5x.
#[test]
fn the_mirror_publishes_the_percentile_because_it_separates_widest() {
    let round = |value: f64| (value * 1000.0).round() / 1000.0;
    let of = |silhouette: Silhouette| {
        let spread = silhouette.reflection().expect("a figure to reflect");
        (round(spread.mean), round(spread.p99), round(spread.worst))
    };
    let committed_view = |name: &str| {
        let image = image::open(committed().join(format!("{name}.png")))
            .expect("a committed view")
            .to_rgb8();
        of(Silhouette::of(&image))
    };

    let front = committed_view("front");
    let back = committed_view("back");
    let stretched = of(Silhouette::of(
        &image::load_from_memory(
            &Concept::view("front")
                .with_the_left_half_stretched(1.15)
                .to_png(),
        )
        .expect("the negative decodes")
        .to_rgb8(),
    ));

    assert_eq!(front, (0.301, 1.167, 4.669), "front: mean, p99, worst");
    assert_eq!(back, (0.286, 1.062, 3.984), "back: mean, p99, worst");
    assert_eq!(stretched, (1.260, 6.394, 6.522), "the negative");
    // And the published limit sits between the worst view and the negative,
    // which is the only property that makes the statistic usable.
    let limit = profile().concept.mirror_percent;
    assert_eq!(limit, 2.4);
    assert!(front.1 < limit && back.1 < limit && stretched.1 > limit);
}

/// The four views are generated one from another, so a view at a different
/// scale is a view the reconstructor fuses into a different body.
#[test]
fn a_side_view_scaled_8_percent_fails_the_cross_view() {
    let broken = Concept::view("left").scaled_by(0.92);
    let dir = with_one_view_replaced("left", &broken);
    let findings = measured(dir.path(), Symmetry::Enforced);

    let read = readings(&findings, "concept.cross_view");
    let round = |value: f64| (value * 1000.0).round() / 1000.0;
    assert_eq!(round(read["front against left"]), 10.549);
    assert_eq!(round(read["back against left"]), 7.299);
    // And the one pair the scale did not touch still reads what it did.
    assert_eq!(round(read["front against back"]), 3.256);
    assert!(
        errors(&findings)
            .iter()
            .any(|finding| finding.rule == "concept.cross_view"),
        "{findings:#?}"
    );
}

// --- the declarations, and the shapes a number cannot carry ---------------

/// `spec.subject.symmetry` is the one declaration that switches a concept
/// rule off, and it switches off exactly one.
#[test]
fn declining_symmetry_switches_the_mirror_off_and_nothing_else() {
    let findings = measured(&committed(), Symmetry::Declined);
    let off: Vec<&Finding> = findings
        .iter()
        .filter(|finding| finding.severity == Severity::Skipped)
        .collect();

    assert_eq!(off.len(), 2, "one per view the mirror owns: {off:#?}");
    for finding in off {
        assert_eq!(finding.rule, "concept.mirror");
        assert_eq!(finding.measured, 0.0, "a skip measured nothing");
        assert!(finding.message.contains("spec.subject.symmetry is false"));
    }
    assert!(errors(&findings).is_empty());
}

/// A missing view is an error under every rule that owns it, never a skip:
/// a gate that goes quiet on absent input proves nothing.
#[test]
fn a_view_that_is_not_an_image_is_an_error_under_every_rule_that_owns_it() {
    let dir = tempfile::tempdir().unwrap();
    for view in View::ALL {
        let file = dir.path().join(format!("{view}.png"));
        if view == View::Front {
            std::fs::write(&file, b"not a png").unwrap();
        } else {
            std::fs::copy(committed().join(format!("{view}.png")), &file).unwrap();
        }
    }
    let findings = measured(dir.path(), Symmetry::Enforced);

    let broken: Vec<&Finding> = findings
        .iter()
        .filter(|finding| finding.subject.contains("front"))
        .collect();
    assert_eq!(
        broken.len(),
        7,
        "four rules on the view, three pairs with it"
    );
    for finding in broken {
        assert_eq!(finding.severity, Severity::Error, "{finding:#?}");
        assert_eq!(finding.unit, "undefined measurements");
        assert!(
            finding.message.contains("holds no readable image"),
            "{finding:#?}"
        );
    }
    // And nothing NaN reached a report.
    for finding in &findings {
        assert!(finding.measured.is_finite() && finding.limit.is_finite());
    }
}

/// And a declaration outranks an unreadable file. `symmetry: false` switched
/// `concept.mirror` off, so it stays `skipped` on a view that holds no image:
/// a rule nobody asked for cannot fail, and it cannot go quiet either.
#[test]
fn a_rule_a_declaration_switched_off_stays_skipped_on_an_unreadable_view() {
    let dir = tempfile::tempdir().unwrap();
    for view in View::ALL {
        std::fs::write(dir.path().join(format!("{view}.png")), b"not a png").unwrap();
    }
    let findings = measured(dir.path(), Symmetry::Declined);
    let mirror: Vec<&Finding> = findings
        .iter()
        .filter(|finding| finding.rule == "concept.mirror")
        .collect();

    assert_eq!(mirror.len(), 2, "one per view the mirror owns");
    for finding in mirror {
        assert_eq!(finding.severity, Severity::Skipped, "{finding:#?}");
        assert!(finding.message.contains("spec.subject.symmetry is false"));
    }
    // Every other rule on the same unreadable views is still an error.
    assert_eq!(
        errors(&findings).len(),
        16,
        "18 findings less the two skips"
    );
}

/// The absent file, which is the other half of the same shape.
#[test]
fn a_view_that_was_never_written_is_an_error_too() {
    let dir = tempfile::tempdir().unwrap();
    let findings = measured(dir.path(), Symmetry::Enforced);

    assert_eq!(findings.len(), 18, "every subject still reports");
    assert_eq!(errors(&findings).len(), 18);
}

// --- the synthetic view every run test generates --------------------------

/// The run tests serve one small synthetic view for all four, so it has to
/// hold every rule. If it stops holding them, the failure belongs here and
/// not in eight end to end tests at once.
#[test]
fn the_synthetic_view_the_run_tests_use_holds_every_rule() {
    let dir = tempfile::tempdir().unwrap();
    for view in View::ALL {
        std::fs::write(
            dir.path().join(format!("{view}.png")),
            crate::support::a_concept_view(),
        )
        .unwrap();
    }
    let findings = measured(dir.path(), Symmetry::Enforced);

    assert!(errors(&findings).is_empty(), "{:#?}", errors(&findings));
    assert_eq!(readings(&findings, "concept.arm_gap")["front"], 100.0);
    assert_eq!(readings(&findings, "concept.mirror")["front"], 0.0);
    assert_eq!(
        readings(&findings, "concept.cross_view")["front against left"],
        0.0
    );
}

/// The tolerance the silhouette is classified at is calibrated too, not
/// picked: from 5 to 25 no view moves by half a percentage point, so 12 sits
/// in the middle of a valley rather than on a slope.
///
/// The share can only fall as the tolerance grows, on any image, so the
/// direction proves nothing. The size of the fall is the whole reading, and
/// the design records the worst of the four at 0.403 points.
#[test]
fn the_background_tolerance_sits_in_a_wide_valley() {
    let mut widest: f64 = 0.0;
    for name in ["front", "back", "left", "right"] {
        let image = image::open(committed().join(format!("{name}.png")))
            .unwrap()
            .to_rgb8();
        let share = |tolerance| Silhouette::at(&image, tolerance).figure_percent();
        let (low, high) = (share(5), share(25));
        assert!(
            low - high <= 0.5,
            "{name}: the figure moves {} points from a tolerance of 5 to 25, so 12 is on a \
             slope and not in a valley",
            low - high
        );
        widest = widest.max(low - high);
    }
    assert_eq!(
        (widest * 1000.0).round() / 1000.0,
        0.402,
        "the worst of the four, which the design records"
    );
}

/// The band [`concept::ARM_GAP`] reads is 25 to 45 percent down the
/// silhouette. On the committed views that lands on rows 384 to 677 and 384
/// to 668, which is where the fixtures record the arms hanging, and is why a
/// gap painted shut over those rows takes the reading to zero.
#[test]
fn the_torso_band_is_the_rows_the_arms_hang_through() {
    for (name, rows) in [("front", (384, 677)), ("back", (384, 668))] {
        let image = image::open(committed().join(format!("{name}.png")))
            .unwrap()
            .to_rgb8();

        assert_eq!(Silhouette::of(&image).torso_band(), Some(rows), "{name}");
    }
}

/// And the gap floor is 4 pixels: three is the seam an anti-aliased edge
/// leaves, four is an arm clear of a ribcage.
///
/// Measured on a drawn figure rather than on the art, because the art has
/// exactly one gap width and cannot show where the floor sits.
#[test]
fn a_gap_of_three_pixels_is_a_seam_and_four_is_an_arm() {
    // A bar with two slits, full height, so the torso band is drawn rows.
    let drawn = |slit: u32| {
        let mut image = image::RgbImage::from_pixel(64, 64, image::Rgb([255, 255, 255]));
        for y in 0..64 {
            for x in 8..56 {
                image.put_pixel(x, y, image::Rgb([0, 0, 0]));
            }
            for gap in [16, 40] {
                for x in gap..gap + slit {
                    image.put_pixel(x, y, image::Rgb([255, 255, 255]));
                }
            }
        }
        Silhouette::of(&image)
            .rows_with_arms_clear()
            .expect("a figure to band")
    };

    assert_eq!(drawn(3), 0.0, "a 3 pixel run is a seam");
    assert_eq!(drawn(4), 100.0, "a 4 pixel run is a gap");
}

/// The speck floor is the other calibrated parameter that publishes no limit:
/// 0.05 percent of the image, 786 pixels of 1024 by 1536. It sits in a gap
/// three orders of magnitude wide, which is what these readings say.
#[test]
fn the_speck_floor_sits_between_a_real_figure_and_every_stray_mark() {
    let sizes: Vec<(u32, u32)> = ["front", "back", "left", "right"]
        .into_iter()
        .map(|name| {
            let image = image::open(committed().join(format!("{name}.png")))
                .unwrap()
                .to_rgb8();
            let pieces = Silhouette::of(&image).pieces();
            (pieces[0], pieces[1])
        })
        .collect();

    assert_eq!(
        sizes,
        [(371_283, 1), (355_935, 2), (219_628, 4), (204_657, 4)],
        "the figure and the next largest piece of each view"
    );
    // 786 pixels, which every figure clears by 260x and every speck misses by
    // 196x.
    let image = image::open(committed().join("front.png"))
        .unwrap()
        .to_rgb8();
    assert_eq!(Silhouette::of(&image).smallest_figure(), 786);
}

/// Every sentence these five rules file, word for word. A report is read by
/// a human, and a number with no sentence around it says nothing about which
/// image to open or what to look at in it.
#[test]
fn every_rule_words_its_finding_the_same_way_every_time() {
    let findings = on_the_committed_views();
    let said = |rule: &str, subject: &str| -> String {
        findings
            .iter()
            .find(|finding| finding.rule == rule && finding.subject == subject)
            .map(|finding| finding.message.clone())
            .unwrap_or_else(|| panic!("{rule} said nothing on {subject}"))
    };

    assert_eq!(
        said("concept.background_flat", "front"),
        "the background of front spans 3 levels over the 76.394 percent of the image the \
         border fill reaches"
    );
    assert_eq!(
        said("concept.single_figure", "front"),
        "front holds 1 figure(s) of at least 786 pixels, out of 7 piece(s), the largest \
         371283 pixels"
    );
    assert_eq!(
        said("concept.arm_gap", "front"),
        "86.054 percent of the torso band of front shows 2 gaps of at least 4 pixels"
    );
    assert_eq!(
        said("concept.mirror", "front"),
        "the 99th percentile row of front sits 1.167 percent of the width from its \
         reflection, with a mean of 0.301 and a worst row of 4.669"
    );
    assert_eq!(
        said("concept.cross_view", "front against back"),
        "front against back: the heights sit 3.256 percent apart and the vertical centroids \
         0.395, of a mean height of 1443.5 pixels"
    );
}
