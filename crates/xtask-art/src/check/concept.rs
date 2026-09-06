//! The concept gates: five rules on the four generated views, read before a
//! single Meshy credit is spent on them.
//!
//! Every reading is taken on the figure's **silhouette**, and the silhouette
//! is found by color rather than by alpha: the generator returns fully opaque
//! PNGs, so all four committed views have a single alpha value of 255 and
//! there is no transparency to separate the subject with. The prompt asks for
//! a flat neutral fill instead, and [`Silhouette::of`] takes it at its word:
//! the background is what the border is made of, and the figure is the rest.
//!
//! A PNG needs no Blender, so these run in Rust, where CI can run them too.

use std::path::Path;

use anyhow::{Context as _, Result};
use image::RgbImage;

use super::profile::Profile;
use super::{Comparison, Finding, NOT_MIRRORED, Rule, Symmetry};

/// The stage these findings belong to, which names their report file.
pub const STAGE: &str = "concept";

/// How far a pixel may sit from the background color, as the largest of its
/// three channel differences.
///
/// Calibrated in `test_concept.rs`: from 5 to 25 the figure keeps the same
/// share of the image to inside half a percentage point, so this sits in the
/// middle of a wide valley rather than on a slope.
const BACKGROUND_TOLERANCE: i32 = 12;

/// A piece of silhouette under this share of the image is a speck rather
/// than a figure: a stray mark, or the anti-aliased rim of one.
///
/// 786 pixels of the 1024 by 1536 the generator returns. Calibrated on the
/// four committed views, where the figure runs from 204,657 pixels to
/// 371,283 and no other piece is over 4.
const SMALLEST_FIGURE_PERCENT: f64 = 0.05;

/// The rows [`ARM_GAP`] reads, as a fraction down the silhouette's own
/// height. The armpit and the hip bracket the only band where an A-posed arm
/// has background on both sides of it.
const TORSO_BAND: (f64, f64) = (0.25, 0.45);

/// A background run narrower than this is not a gap between an arm and a
/// ribcage: it is the seam an anti-aliased edge leaves behind.
const SMALLEST_GAP_PIXELS: u32 = 4;

/// How many gaps a row of the torso band shows when both arms are clear of
/// the body: one on the left of the torso and one on its right.
const ARMS_CLEAR: usize = 2;

const OUTSIDE: &str = "the pixels a border flood fill reaches, the widest of the three channel \
                       spreads from the 1st to the 99th percentile, in levels of 0 to 255";
const PIECES: &str = "the 4-connected pieces of one view's silhouette, specks under 0.05 percent \
                      of the image dropped";
const BAND: &str = "the rows 25 to 45 percent down one view's silhouette, counting the background \
                    runs between its leftmost and rightmost figure pixel";
const REFLECTED: &str = "each row's leftmost and rightmost figure pixel of one view's \
                         silhouette against its reflection, the 99th percentile over the rows, \
                         as a percent of the silhouette's width";
const PAIRED: &str = "two views' silhouettes, their heights and their vertical centroids, as a \
                      percent of the mean of the two heights";

/// How even the background is. The reconstructor segments the figure off this
/// fill, so a gradient in it is a gradient in the mesh's outline.
pub const BACKGROUND_FLAT: Rule = Rule {
    id: "concept.background_flat",
    comparison: Comparison::Le,
    unit: "levels",
    space: OUTSIDE,
    limit: |profile| profile.concept.background_spread_levels,
};

/// One figure, not two. A second character in the frame is reconstructed
/// into the same mesh as the first.
pub const SINGLE_FIGURE: Rule = Rule {
    id: "concept.single_figure",
    comparison: Comparison::Eq,
    unit: "figures",
    space: PIECES,
    limit: |_| 1.0,
};

/// Whether the arms are clear of the torso. An arm painted onto the ribcage
/// is reconstructed as part of it, and no amount of rigging separates them
/// afterwards.
pub const ARM_GAP: Rule = Rule {
    id: "concept.arm_gap",
    comparison: Comparison::Ge,
    unit: "percent",
    space: BAND,
    limit: |profile| profile.concept.arm_gap_rows_percent,
};

/// How far a view sits from its own reflection.
pub const MIRROR: Rule = Rule {
    id: "concept.mirror",
    comparison: Comparison::Le,
    unit: "percent",
    space: REFLECTED,
    limit: |profile| profile.concept.mirror_percent,
};

/// Whether the four views show one character at one size. They are generated
/// one from another, so a view at a different scale is a view the
/// reconstructor fuses into a different body.
pub const CROSS_VIEW: Rule = Rule {
    id: "concept.cross_view",
    comparison: Comparison::Le,
    unit: "percent",
    space: PAIRED,
    limit: |profile| profile.concept.cross_view_percent,
};

/// Every concept rule, in the order `--list-rules` prints them.
pub const RULES: [&Rule; 5] = [
    &BACKGROUND_FLAT,
    &SINGLE_FIGURE,
    &ARM_GAP,
    &MIRROR,
    &CROSS_VIEW,
];

/// One generated view: the file, the name every finding on it carries, and
/// whether the torso faces the camera.
///
/// Only a view that faces the camera can show a gap between an arm and the
/// ribcage, or a left half to read against a right, so [`ARM_GAP`] and
/// [`MIRROR`] own those views and no others.
#[derive(Debug, Clone, Copy)]
pub struct Rendered<'a> {
    pub name: &'a str,
    pub file: &'a Path,
    pub torso_faces_the_camera: bool,
}

/// Runs every concept rule over a set of views.
///
/// A view that holds no readable image is an error under every rule that owns
/// it, never a skip: a gate that goes quiet on absent input proves nothing.
pub fn check_files(
    views: &[Rendered<'_>],
    profile: &Profile,
    symmetry: Symmetry,
    attempt: u32,
) -> Vec<Finding> {
    let read: Vec<(&Rendered<'_>, Result<Silhouette, String>)> = views
        .iter()
        .map(|view| {
            let silhouette = Silhouette::read(view.file).map_err(|error| {
                format!("{} holds no readable image: {error:#}", view.file.display())
            });
            (view, silhouette)
        })
        .collect();

    let mut findings = Vec::new();
    for (view, silhouette) in &read {
        findings.extend(one_view(
            view,
            silhouette.as_ref(),
            profile,
            symmetry,
            attempt,
        ));
    }
    for (index, (left, left_silhouette)) in read.iter().enumerate() {
        for (right, right_silhouette) in read.iter().skip(index + 1) {
            let subject = paired(left.name, right.name);
            findings.push(
                match (left_silhouette.as_ref(), right_silhouette.as_ref()) {
                    (Ok(a), Ok(b)) => cross_view(&subject, a, b, profile, attempt),
                    (Err(reason), _) | (_, Err(reason)) => {
                        CROSS_VIEW.undefined(&subject, attempt, reason.clone())
                    }
                },
            );
        }
    }
    findings
}

/// Every subject a set of views owes a finding on: each view, and each pair
/// of them. The runner refuses a report that names none of its findings after
/// one of these, so a view dropped from the pass is a failing stage.
pub fn subjects(views: &[Rendered<'_>]) -> Vec<String> {
    let mut named: Vec<String> = views.iter().map(|view| view.name.to_owned()).collect();
    for (index, left) in views.iter().enumerate() {
        for right in views.iter().skip(index + 1) {
            named.push(paired(left.name, right.name));
        }
    }
    named
}

/// What [`CROSS_VIEW`] calls one pair of views.
fn paired(left: &str, right: &str) -> String {
    format!("{left} against {right}")
}

/// The rules read on one view alone.
fn one_view(
    view: &Rendered<'_>,
    silhouette: Result<&Silhouette, &String>,
    profile: &Profile,
    symmetry: Symmetry,
    attempt: u32,
) -> Vec<Finding> {
    let mut findings = match silhouette {
        Ok(read) => vec![
            background_flat(view.name, read, profile, attempt),
            single_figure(view.name, read, profile, attempt),
        ],
        Err(reason) => vec![
            BACKGROUND_FLAT.undefined(view.name, attempt, reason.clone()),
            SINGLE_FIGURE.undefined(view.name, attempt, reason.clone()),
        ],
    };
    if view.torso_faces_the_camera {
        findings.push(match silhouette {
            Ok(read) => arm_gap(view.name, read, profile, attempt),
            Err(reason) => ARM_GAP.undefined(view.name, attempt, reason.clone()),
        });
        // A declaration outranks an unreadable file: a rule the spec switched
        // off stays `skipped` whatever the file turns out to hold.
        findings.push(match (symmetry, silhouette) {
            (Symmetry::Declined, _) => {
                MIRROR.skipped(profile, view.name, attempt, NOT_MIRRORED.to_owned())
            }
            (Symmetry::Enforced, Ok(read)) => mirror(view.name, read, profile, attempt),
            (Symmetry::Enforced, Err(reason)) => {
                MIRROR.undefined(view.name, attempt, reason.clone())
            }
        });
    }
    findings
}

fn background_flat(
    name: &str,
    silhouette: &Silhouette,
    profile: &Profile,
    attempt: u32,
) -> Finding {
    // The fill is all this rule sees, so the share it covers is part of the
    // reading: a second backdrop tone the fill stopped at is not in the spread.
    let filled = 100.0 - silhouette.figure_percent();
    match silhouette.background_spread() {
        Some(spread) => BACKGROUND_FLAT.measured(
            profile,
            name,
            f64::from(spread),
            attempt,
            format!(
                "the background of {name} spans {spread} levels over the {filled:.3} percent of \
                 the image the border fill reaches"
            ),
        ),
        None => BACKGROUND_FLAT.undefined(
            name,
            attempt,
            format!("no pixel of {name} is within {BACKGROUND_TOLERANCE} levels of its own border, so it has no background to read"),
        ),
    }
}

fn single_figure(name: &str, silhouette: &Silhouette, profile: &Profile, attempt: u32) -> Finding {
    let pieces = silhouette.pieces();
    let figures = pieces
        .iter()
        .filter(|area| **area >= silhouette.smallest_figure())
        .count();
    SINGLE_FIGURE.measured(
        profile,
        name,
        figures as f64,
        attempt,
        format!(
            "{name} holds {figures} figure(s) of at least {} pixels, out of {} piece(s), the largest {} pixels",
            silhouette.smallest_figure(),
            pieces.len(),
            pieces.first().copied().unwrap_or_default(),
        ),
    )
}

fn arm_gap(name: &str, silhouette: &Silhouette, profile: &Profile, attempt: u32) -> Finding {
    match silhouette.rows_with_arms_clear() {
        Some(percent) => ARM_GAP.measured(
            profile,
            name,
            percent,
            attempt,
            format!(
                "{percent:.3} percent of the torso band of {name} shows {ARMS_CLEAR} gaps of at \
                 least {SMALLEST_GAP_PIXELS} pixels"
            ),
        ),
        None => ARM_GAP.undefined(
            name,
            attempt,
            format!("{name} holds no figure, so it has no torso band"),
        ),
    }
}

fn mirror(name: &str, silhouette: &Silhouette, profile: &Profile, attempt: u32) -> Finding {
    match silhouette.reflection() {
        Some(spread) => MIRROR.measured(
            profile,
            name,
            spread.p99,
            attempt,
            format!(
                "the 99th percentile row of {name} sits {:.3} percent of the width from its \
                 reflection, with a mean of {:.3} and a worst row of {:.3}",
                spread.p99, spread.mean, spread.worst
            ),
        ),
        None => MIRROR.undefined(
            name,
            attempt,
            format!("{name} holds no figure, so it has nothing to reflect"),
        ),
    }
}

fn cross_view(
    subject: &str,
    left: &Silhouette,
    right: &Silhouette,
    profile: &Profile,
    attempt: u32,
) -> Finding {
    let Some((a, b)) = left.stature().zip(right.stature()) else {
        return CROSS_VIEW.undefined(
            subject,
            attempt,
            format!("{subject}: one of the two holds no figure, so they have nothing to agree on"),
        );
    };
    let mean = (a.height + b.height) / 2.0;
    let percent = |apart: f64| apart / mean * 100.0;
    let (heights, centroids) = (
        percent((a.height - b.height).abs()),
        percent((a.centroid - b.centroid).abs()),
    );
    CROSS_VIEW.measured(
        profile,
        subject,
        heights.max(centroids),
        attempt,
        format!(
            "{subject}: the heights sit {heights:.3} percent apart and the vertical centroids \
             {centroids:.3}, of a mean height of {mean:.1} pixels"
        ),
    )
}

/// One view's figure, separated from its background.
pub struct Silhouette {
    width: u32,
    height: u32,
    /// True where the pixel belongs to the figure.
    figure: Vec<bool>,
    /// How many background pixels carry each level, per channel.
    background: [[u32; LEVELS]; 3],
}

/// The values one 8-bit channel can hold.
const LEVELS: usize = 256;

impl Silhouette {
    pub fn read(file: &Path) -> Result<Self> {
        let image = image::open(file).with_context(|| format!("reading {}", file.display()))?;
        Ok(Self::of(&image.to_rgb8()))
    }

    /// Separates the figure from the background.
    ///
    /// The background color is the median of the image's own one-pixel
    /// border, and the background is what a flood fill from there reaches
    /// within a calibrated tolerance. Filling rather than thresholding is
    /// what keeps an enclosed patch of backdrop, between two fingers or under
    /// an arm, part of the figure that encloses it.
    pub fn of(image: &RgbImage) -> Self {
        Self::at(image, BACKGROUND_TOLERANCE)
    }

    /// The same, at a tolerance the caller chooses. Only the calibration of
    /// that tolerance passes anything but the published one.
    pub fn at(image: &RgbImage, tolerance: i32) -> Self {
        let (width, height) = (image.width(), image.height());
        let backdrop = border_median(image);
        let at = |x: u32, y: u32| (y * width + x) as usize;
        let matches = |x: u32, y: u32| {
            let pixel = image.get_pixel(x, y).0;
            (0..3)
                .map(|channel| (i32::from(pixel[channel]) - backdrop[channel]).abs())
                .all(|apart| apart <= tolerance)
        };

        let mut figure = vec![true; (width * height) as usize];
        let mut stack: Vec<(u32, u32)> = Vec::new();
        let seed = |x: u32, y: u32, figure: &mut Vec<bool>, stack: &mut Vec<(u32, u32)>| {
            if figure[at(x, y)] && matches(x, y) {
                figure[at(x, y)] = false;
                stack.push((x, y));
            }
        };
        for x in 0..width {
            seed(x, 0, &mut figure, &mut stack);
            seed(x, height - 1, &mut figure, &mut stack);
        }
        for y in 0..height {
            seed(0, y, &mut figure, &mut stack);
            seed(width - 1, y, &mut figure, &mut stack);
        }
        while let Some((x, y)) = stack.pop() {
            for (nx, ny) in neighbors(x, y, width, height) {
                seed(nx, ny, &mut figure, &mut stack);
            }
        }

        let mut background = [[0u32; LEVELS]; 3];
        for (index, pixel) in image.pixels().enumerate() {
            if !figure[index] {
                for channel in 0..3 {
                    background[channel][usize::from(pixel.0[channel])] += 1;
                }
            }
        }
        Self {
            width,
            height,
            figure,
            background,
        }
    }

    /// How uneven the background is: the widest of the three channels' 1st to
    /// 99th percentile spreads. `None` when nothing was classed as
    /// background.
    pub fn background_spread(&self) -> Option<u32> {
        let total = u64::from(self.background[0].iter().sum::<u32>());
        if total == 0 {
            return None;
        }
        (0..3)
            .map(|channel| {
                let level = |fraction| percentile(&self.background[channel], total, fraction);
                u32::from(level(0.99)) - u32::from(level(0.01))
            })
            .max()
    }

    /// What share of the image is figure, as a percent.
    pub fn figure_percent(&self) -> f64 {
        let figure = self.figure.iter().filter(|figure| **figure).count();
        figure as f64 / self.figure.len() as f64 * 100.0
    }

    /// Every connected piece of the figure, by pixel count, largest first.
    pub fn pieces(&self) -> Vec<u32> {
        let mut seen = vec![false; self.figure.len()];
        let mut areas = Vec::new();
        for y in 0..self.height {
            for x in 0..self.width {
                let start = (y * self.width + x) as usize;
                if seen[start] || !self.figure[start] {
                    continue;
                }
                seen[start] = true;
                let mut stack = vec![(x, y)];
                let mut area = 0;
                while let Some((x, y)) = stack.pop() {
                    area += 1;
                    for (nx, ny) in neighbors(x, y, self.width, self.height) {
                        let index = (ny * self.width + nx) as usize;
                        if !seen[index] && self.figure[index] {
                            seen[index] = true;
                            stack.push((nx, ny));
                        }
                    }
                }
                areas.push(area);
            }
        }
        areas.sort_unstable_by(|a, b| b.cmp(a));
        areas
    }

    /// The smallest piece that counts as a figure rather than a speck.
    pub fn smallest_figure(&self) -> u32 {
        (f64::from(self.width * self.height) * SMALLEST_FIGURE_PERCENT / 100.0) as u32
    }

    /// What share of the torso band's rows show both arms clear of the body,
    /// as a percent. `None` when there is no figure to band.
    pub fn rows_with_arms_clear(&self) -> Option<f64> {
        let (top, bottom) = self.torso_band()?;
        let clear = (top..=bottom)
            .filter(|y| self.interior_gaps(*y) >= ARMS_CLEAR)
            .count();
        Some(clear as f64 / f64::from(bottom - top + 1) * 100.0)
    }

    /// How many runs of background of at least [`SMALLEST_GAP_PIXELS`] sit
    /// between the leftmost and the rightmost figure pixel of one row.
    fn interior_gaps(&self, y: u32) -> usize {
        let Some((left, right)) = self.row_span(y) else {
            return 0;
        };
        let mut gaps = 0;
        let mut run = 0;
        for x in left..=right {
            if self.figure[(y * self.width + x) as usize] {
                if run >= SMALLEST_GAP_PIXELS {
                    gaps += 1;
                }
                run = 0;
            } else {
                run += 1;
            }
        }
        gaps
    }

    /// The band [`ARM_GAP`] reads, in image coordinates: 25 to 45 percent
    /// down the silhouette. `None` when there is no figure to band.
    pub fn torso_band(&self) -> Option<(u32, u32)> {
        let (top, bottom) = self.vertical_span()?;
        let span = f64::from(bottom - top);
        let row = |fraction: f64| top + (span * fraction) as u32;
        Some((row(TORSO_BAND.0), row(TORSO_BAND.1)))
    }

    /// How far the silhouette sits from its own reflection.
    pub fn reflection(&self) -> Option<Reflection> {
        let (top, bottom) = self.vertical_span()?;
        let (left, right) = self.horizontal_span()?;
        let midpoints: Vec<f64> = (top..=bottom)
            .filter_map(|y| self.row_span(y))
            .map(|(a, b)| f64::from(a + b) / 2.0)
            .collect();
        // The median midpoint, which is the axis that minimizes the readings
        // below, so a figure standing off center is not called asymmetric for
        // standing there.
        let mut sorted = midpoints.clone();
        sorted.sort_unstable_by(f64::total_cmp);
        let axis = sorted[sorted.len() / 2];
        let width = f64::from(right - left + 1);
        // Twice the midpoint's drift is how far the two half-extents of a row
        // sit apart, which is what a longer arm on one side moves.
        let mut apart: Vec<f64> = midpoints
            .iter()
            .map(|mid| (mid - axis).abs() * 2.0 / width * 100.0)
            .collect();
        let mean = apart.iter().sum::<f64>() / apart.len() as f64;
        apart.sort_unstable_by(f64::total_cmp);
        Some(Reflection {
            mean,
            p99: apart[apart.len() * 99 / 100],
            worst: *apart.last()?,
        })
    }

    /// The figure's height and where its weight sits, both in pixels.
    pub fn stature(&self) -> Option<Stature> {
        let (top, bottom) = self.vertical_span()?;
        let mut weight = 0u64;
        let mut moment = 0u64;
        for y in top..=bottom {
            let row = (0..self.width)
                .filter(|x| self.figure[(y * self.width + x) as usize])
                .count() as u64;
            weight += row;
            moment += row * u64::from(y);
        }
        Some(Stature {
            height: f64::from(bottom - top + 1),
            centroid: moment as f64 / weight as f64,
        })
    }

    /// The first and last rows holding any figure pixel.
    fn vertical_span(&self) -> Option<(u32, u32)> {
        let mut rows = (0..self.height).filter(|y| self.row_span(*y).is_some());
        let top = rows.next()?;
        Some((top, rows.next_back().unwrap_or(top)))
    }

    /// The leftmost and rightmost columns holding any figure pixel.
    fn horizontal_span(&self) -> Option<(u32, u32)> {
        let spans: Vec<(u32, u32)> = (0..self.height).filter_map(|y| self.row_span(y)).collect();
        let left = spans.iter().map(|(left, _)| *left).min()?;
        let right = spans.iter().map(|(_, right)| *right).max()?;
        Some((left, right))
    }

    /// The leftmost and rightmost figure pixel of one row.
    fn row_span(&self, y: u32) -> Option<(u32, u32)> {
        let row = &self.figure[(y * self.width) as usize..((y + 1) * self.width) as usize];
        let left = row.iter().position(|figure| *figure)? as u32;
        let right = row.iter().rposition(|figure| *figure)? as u32;
        Some((left, right))
    }
}

/// How far a silhouette's rows sit from their own reflection, in percent of
/// the silhouette's width.
///
/// [`MIRROR`] publishes the 99th percentile, as `mesh.mirror` publishes a
/// tail statistic too. The outright worst row is hair: on the committed front
/// view it reads 4.669 against the 6.522 a stretched half reads, 1.4x apart,
/// where the percentile reads 1.167 against 6.394.
#[derive(Debug, Clone, Copy)]
pub struct Reflection {
    pub mean: f64,
    pub p99: f64,
    pub worst: f64,
}

/// One silhouette's size and placement in its frame, in pixels.
#[derive(Debug, Clone, Copy)]
pub struct Stature {
    pub height: f64,
    /// The mean row of the figure's own pixels, weighted by how many each row
    /// holds.
    pub centroid: f64,
}

/// The median color of the image's one-pixel border, per channel.
fn border_median(image: &RgbImage) -> [i32; 3] {
    let (width, height) = (image.width(), image.height());
    let ring: Vec<[u8; 3]> = (0..width)
        .flat_map(|x| [image.get_pixel(x, 0).0, image.get_pixel(x, height - 1).0])
        .chain((0..height).flat_map(|y| [image.get_pixel(0, y).0, image.get_pixel(width - 1, y).0]))
        .collect();
    [0, 1, 2].map(|channel| {
        let mut levels: Vec<u8> = ring.iter().map(|pixel| pixel[channel]).collect();
        levels.sort_unstable();
        i32::from(levels[levels.len() / 2])
    })
}

/// The level at which `fraction` of a histogram's weight has been counted.
fn percentile(histogram: &[u32; LEVELS], total: u64, fraction: f64) -> u8 {
    let target = (total as f64 * fraction).ceil() as u64;
    let mut counted = 0u64;
    for (level, count) in histogram.iter().enumerate() {
        counted += u64::from(*count);
        if counted >= target {
            return level as u8;
        }
    }
    u8::MAX
}

/// The pixels sharing an edge with this one, inside the image.
fn neighbors(x: u32, y: u32, width: u32, height: u32) -> impl Iterator<Item = (u32, u32)> {
    [
        (x > 0).then(|| (x - 1, y)),
        (y > 0).then(|| (x, y - 1)),
        (x + 1 < width).then_some((x + 1, y)),
        (y + 1 < height).then_some((x, y + 1)),
    ]
    .into_iter()
    .flatten()
}
