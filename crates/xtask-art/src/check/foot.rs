//! Where a foot touches the ground, measured on the delivered clip.
//!
//! The Rust twin of `tools/blender/src/plant.py`, and for the reason rule
//! four gives: the retarget reads the pose it evaluated, which is the only
//! place the lock itself is visible, and this reads the file that was written
//! from it. Between the two sit the interpolation pass and the exporter.
//! Running here is also what gives all three rules a negative control in CI,
//! which has no Blender.
//!
//! Every rule here reads a **sole point** and never the toe joint, two per
//! foot, under the ankle and under the toe. Corrections 1 and 2 of T9 in the
//! design have the measurements that say why.
//!
//! # Why the thresholds scale
//!
//! Both are published against a 180 cm reference and multiplied by the rig's
//! own joint span, so a rig of another size is read against the same shape of
//! motion. The speed one is meters per second rather than meters per frame,
//! so a clip means the same thing at 8 fps and at 30.

use glam::DVec3;

use super::gltf_clip::Soles;
use super::profile::Profile;
use super::{Comparison, Finding, Rule};

/// The height every threshold below is published against, in meters.
pub const REFERENCE_HEIGHT_METERS: f64 = 1.80;

/// How near the ground a sole is before it counts as touching it.
pub const CONTACT_HEIGHT_METERS: f64 = 0.03;

/// And how slowly it must be moving, as a rate.
pub const CONTACT_SPEED_MPS: f64 = 0.30;

/// How wide the majority window is at 60 fps, scaled to the clip's own rate.
const VOTE_FRAMES_AT_60: f64 = 5.0;

const CONTACT: &str = "each foot's sole point under the toe, at every frame of the clip, in \
                       Blender Z-up world space, against the ground plane at zero";

const STANCE: &str = "each foot's sole point under the toe, inside one plant run, against \
                      where the run's first frame put it, horizontally, in Blender Z-up \
                      world space";

const SOLE: &str = "the two sole points of each foot, under the ankle and under the toe, at \
                    every frame, against the ground plane at zero, in Blender Z-up world space";

/// What a message calls each of the two sole points. `plant.py` spells them
/// the same, so one reading names one place whichever side took it.
const UNDER_THE_TOE: &str = "under the toe";
const UNDER_THE_ANKLE: &str = "under the ankle";

/// Every foot comes to rest at least once a cycle.
///
/// An empty set has no maximum worth trusting, so zero runs is an error here
/// rather than a [`SKATE`] of 0.0. A clip the library declares in place is
/// switched off instead: its ground moves under it, so its feet must slide.
pub const PLANTS: Rule = Rule {
    id: "clip.foot_contact.plants",
    comparison: Comparison::Ge,
    unit: "plant runs",
    space: CONTACT,
    limit: |profile| profile.clip.foot_plants,
};

/// And it does not slide while it is down.
///
/// The largest distance the sole gets from where the run's first frame put
/// it, horizontally, per run. This is the one clip rule that reads a planted
/// foot against the ground rather than a bone against the source, so it does
/// not go through the femur ratio at all.
pub const SKATE: Rule = Rule {
    id: "clip.foot_contact.skate",
    comparison: Comparison::Le,
    unit: "meters",
    space: STANCE,
    limit: |profile| profile.clip.foot_skate_meters,
};

/// And no part of it goes through the floor.
///
/// The true sole datum, where [`super::clip::FLOOR_SNAP`]'s is the toe
/// joint's own rest height. Negative when the sole never reaches the floor at
/// all, which is the clearance it kept: a clip that never lands is a defect
/// [`PLANTS`] owns, and reporting it as a depth of zero here would hide it.
pub const PENETRATION: Rule = Rule {
    id: "clip.foot_contact.penetration",
    comparison: Comparison::Le,
    unit: "meters",
    space: SOLE,
    limit: |profile| profile.clip.foot_penetration_meters,
};

/// Every rule this module owns, in the order `--list-rules` prints them.
pub const RULES: [&Rule; 3] = [&PLANTS, &SKATE, &PENETRATION];

/// How much bigger this rig is than the rig the thresholds are stated on.
/// `None` for a rig with no height, which scales nothing.
pub fn scale_of(height: f64) -> Option<f64> {
    (height.is_finite() && height > 0.0).then(|| height / REFERENCE_HEIGHT_METERS)
}

/// The majority window at one clip's own rate, odd and at least 3.
///
/// Five frames at 60 fps, so the window covers the same slice of time
/// whatever the rate. `| 1` makes it odd, because an even window can tie and
/// a tie is not a majority. It is also what makes this agree with Python's
/// `round`, which breaks a half to even where Rust's breaks it away from
/// zero: the two differ by one only at a half, and both are then odd.
pub fn vote_width(source_fps: u32) -> usize {
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let scaled = (VOTE_FRAMES_AT_60 * f64::from(source_fps) / 60.0).round() as usize;
    (scaled | 1).max(3)
}

/// A majority vote over a window of `width` frames, centered on each.
///
/// The clip is held at its ends rather than the window shrinking, so every
/// frame is decided by the same number of votes.
///
/// # Panics
///
/// On a width that carries no majority, which is an even one or one under 3.
/// An even window is not centered on its frame either, so the answer would
/// be wrong rather than merely tied. `plant.py::voted` refuses the same set.
pub fn voted(flags: &[bool], width: usize) -> Vec<bool> {
    assert!(
        width >= 3 && width % 2 == 1,
        "a majority window is odd and at least 3, got {width}"
    );
    let (last, half) = (flags.len().saturating_sub(1), width / 2);
    (0..flags.len())
        .map(|at| {
            let held = (0..width)
                .filter(|step| flags[(at + step).saturating_sub(half).min(last)])
                .count();
            held * 2 > width
        })
        .collect()
}

/// Every stretch of consecutive contact frames, as index pairs.
fn runs_of(flags: &[bool]) -> Vec<(usize, usize)> {
    let mut found: Vec<(usize, usize)> = Vec::new();
    for (at, _) in flags.iter().enumerate().filter(|(_, held)| **held) {
        match found.last_mut() {
            Some(run) if run.1 + 1 == at => run.1 = at,
            _ => found.push((at, at)),
        }
    }
    found
}

/// The frames one foot is planted for: low, slow, and voted on.
///
/// The speed is the step to the neighboring frame times the clip's own rate,
/// so it is meters per second. The first frame has no previous one and is
/// read against the next: read against itself it would be still by
/// construction, and a foot that starts low and leaves at once would plant
/// for exactly one frame.
pub fn plant_runs(path: &[DVec3], source_fps: u32, scale: f64) -> Vec<(usize, usize)> {
    let (ceiling, limit) = (CONTACT_HEIGHT_METERS * scale, CONTACT_SPEED_MPS * scale);
    let beside = |at: usize| at.checked_sub(1).unwrap_or(1.min(path.len() - 1));
    let touching: Vec<bool> = path
        .iter()
        .enumerate()
        .map(|(at, point)| {
            let step = (point.truncate() - path[beside(at)].truncate()).length();
            point.z < ceiling && step * f64::from(source_fps) < limit
        })
        .collect();
    runs_of(&voted(&touching, vote_width(source_fps)))
}

/// How far the sole wanders from where the run's first frame put it.
pub fn drift(path: &[DVec3], run: (usize, usize)) -> f64 {
    path[run.0..=run.1]
        .iter()
        .map(|point| (point.truncate() - path[run.0].truncate()).length())
        .fold(0.0, f64::max)
}

/// All three rules on one delivered clip, one foot at a time.
///
/// `height` is the joint span of the rig this clip was fitted to, which is
/// the same file and the same set `retarget_animation.py::rig_height` spans.
pub fn check(
    clip: &str,
    soles: &Soles,
    height: f64,
    source_fps: u32,
    travels: bool,
    profile: &Profile,
    attempt: u32,
) -> Vec<Finding> {
    let (Some(scale), true) = (scale_of(height), source_fps > 0) else {
        return undefined(
            clip,
            attempt,
            &format!(
                "a rig {height:.4} m tall at {source_fps} fps has no scale for \
                 the contact thresholds"
            ),
        );
    };
    soles
        .feet
        .iter()
        .flat_map(|foot| {
            let runs = plant_runs(&foot.ball, source_fps, scale);
            [
                vec![plants(&foot.toe, &runs, travels, profile, attempt)],
                skate(&foot.toe, &runs, &foot.ball, travels, profile, attempt),
                vec![penetration(foot, &soles.seconds, profile, attempt)],
            ]
            .concat()
        })
        .collect()
}

/// One undefined finding per rule, for a file none of them can be read on.
pub fn undefined(clip: &str, attempt: u32, why: &str) -> Vec<Finding> {
    RULES
        .map(|rule| rule.undefined(clip, attempt, why.to_owned()))
        .to_vec()
}

/// `clip.foot_contact.plants`: how often one foot comes to rest.
fn plants(
    foot: &str,
    runs: &[(usize, usize)],
    travels: bool,
    profile: &Profile,
    attempt: u32,
) -> Finding {
    if !travels {
        return PLANTS.skipped(profile, foot, attempt, in_place(foot));
    }
    if runs.is_empty() {
        return PLANTS.measured(
            profile,
            foot,
            0.0,
            attempt,
            format!("{foot} never comes to rest on the ground over the clip"),
        );
    }
    let stood = runs
        .iter()
        .map(|(start, end)| format!("{start}..{end}"))
        .collect::<Vec<String>>()
        .join(", ");
    PLANTS.measured(
        profile,
        foot,
        runs.len() as f64,
        attempt,
        format!(
            "{foot} plants {} time(s) over the clip, on frames {stood}",
            runs.len()
        ),
    )
}

/// `clip.foot_contact.skate`: how far a planted foot slides, per run.
fn skate(
    foot: &str,
    runs: &[(usize, usize)],
    path: &[DVec3],
    travels: bool,
    profile: &Profile,
    attempt: u32,
) -> Vec<Finding> {
    if !travels {
        return vec![SKATE.skipped(profile, foot, attempt, in_place(foot))];
    }
    if runs.is_empty() {
        return vec![SKATE.undefined(
            foot,
            attempt,
            format!("{foot} never plants, so it has no stance to be read across"),
        )];
    }
    runs.iter()
        .enumerate()
        .map(|(index, run)| {
            let slid = drift(path, *run);
            SKATE.measured(
                profile,
                &format!("{foot} run {}", index + 1),
                slid,
                attempt,
                format!(
                    "{foot} drifts {slid:.4} m over frames {}..{}, where it is planted",
                    run.0, run.1
                ),
            )
        })
        .collect()
}

/// `clip.foot_contact.penetration`: how far the sole gets under the floor.
fn penetration(
    foot: &super::gltf_clip::Sole,
    seconds: &[f64],
    profile: &Profile,
    attempt: u32,
) -> Finding {
    // Both points, so the message names the one that sank rather than a
    // depth with no place. `min_by` keeps the first of equal readings, so
    // two points at one height name the toe.
    let lowest = foot
        .ball
        .iter()
        .zip(&foot.heel)
        .enumerate()
        .flat_map(|(at, (ball, heel))| [(at, ball.z, UNDER_THE_TOE), (at, heel.z, UNDER_THE_ANKLE)])
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let Some((at, low, point)) = lowest else {
        return PENETRATION.undefined(
            &foot.toe,
            attempt,
            format!(
                "{} has no frame at all, so its sole has no height to read",
                foot.toe
            ),
        );
    };
    let when = seconds[at];
    PENETRATION.measured(
        profile,
        &foot.toe,
        -low,
        attempt,
        if low < 0.0 {
            format!(
                "the sole of {} {point} sinks {:.4} m below the ground at {when:.6} s",
                foot.toe, -low
            )
        } else {
            format!(
                "the sole of {} {point} gets to {low:.4} m over the ground at \
                 {when:.6} s",
                foot.toe
            )
        },
    )
}

/// Why a clip the library declares in place has no plant to read.
fn in_place(foot: &str) -> String {
    format!(
        "the library declares travels: false, so {foot} stands on ground that \
         moves under it and cannot plant"
    )
}
