//! Where a clip's bones pointed, frame by frame, in one space both sides
//! agree on.
//!
//! `clip.swing` and `clip.twist` are absolute rules: they read the delivered
//! GLB against the file the motion was bought in. Two readers therefore
//! produce this one record.
//!
//! - Our own output is a GLB, so [`super::gltf_clip`] reads it, which is what
//!   makes a wrong export visible.
//! - The vendor file is an FBX. No Rust reader opens one and CI has no
//!   Blender, so `retarget_animation.py` records what it read into a sidecar
//!   beside the report and [`Motion::read`] parses it back.
//!
//! Everything here is **Blender Z-up world space**, which is the space the
//! aim table and `rig.aim_table` already state. The conversion is not
//! optional decoration: a twist about a bone's own axis is not invariant
//! under a change of world frame, so measuring one side in glTF Y-up and the
//! other in Blender would produce precise, wrong numbers.
//!
//! [`Motion`] is rotations only. Both rules read where a bone points and how
//! far it is rolled about its own length, and neither reads a position.
//!
//! The same sidecar carries two lengths beside them, in [`SourceLengths`],
//! because `clip.stride` and `clip.stride_ratio` also measure the fit against
//! a file no Rust reader opens. They are a separate type read from the same
//! schema, so neither rule can be handed the other's numbers.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context as _, Result, ensure};
use glam::DQuat;
use serde::Deserialize;

/// One instant of a clip.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Seconds from the clip's own first frame.
    ///
    /// Normalized on the way in, so the two sides align on elapsed time
    /// rather than on a frame number or an index. A Mixamo clip runs frames 1
    /// to 21 and a Meshy one starts at 0, and neither side is told the
    /// other's numbering.
    pub seconds: f64,
    /// Role to that bone's world rotation.
    pub rotations: BTreeMap<String, DQuat>,
}

/// One clip's orientations: the rest pose it was authored on, and every frame.
#[derive(Debug, Clone)]
pub struct Motion {
    rest: BTreeMap<String, DQuat>,
    frames: Vec<Frame>,
}

/// The sidecar as `clip.py` writes it. Quaternions are `w, x, y, z`, which is
/// Blender's own order and the order `transfer.py` works in.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceMotionFile {
    rest: BTreeMap<String, [f64; 4]>,
    frames: Vec<FrameFile>,
    travel: f64,
    stride_segment: f64,
}

/// The two lengths of the source clip, in meters, which no rotation carries.
///
/// `clip.stride` holds the fit's own travel to the source's sized by the
/// femur, and `clip.stride_ratio` records that ratio, so both need a length
/// out of the FBX. Read here rather than hung off [`Motion`] because a
/// [`Motion`] read out of the delivered GLB has neither.
#[derive(Debug, Clone, Copy)]
pub struct SourceLengths {
    /// How far the source's own root got from where it started,
    /// horizontally. The same reading `source.traveling` takes at the fetch.
    pub travel: f64,
    /// The source rig's two `stride_segment` joints at rest, apart.
    pub stride_segment: f64,
}

impl SourceLengths {
    pub fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading the source motion {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("in {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let file: SourceMotionFile =
            serde_json::from_str(text).context("parsing the source motion")?;
        ensure!(
            file.travel.is_finite() && file.travel >= 0.0,
            "the source travels {} m, which is no distance at all",
            file.travel
        );
        // Zero would make the femur ratio a division by nothing, and the
        // rule that divides is three functions away from this file.
        ensure!(
            file.stride_segment.is_finite() && file.stride_segment > 0.0,
            "the source's stride segment is {} m, so no ratio can be taken \
             against it",
            file.stride_segment
        );
        Ok(Self {
            travel: file.travel,
            stride_segment: file.stride_segment,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FrameFile {
    seconds: f64,
    rotations: BTreeMap<String, [f64; 4]>,
}

impl Motion {
    /// One clip, validated and with its times taken to the clip's own start.
    ///
    /// A frame that carries a different role set from the rest pose is
    /// refused rather than measured on whatever the two have in common: a
    /// rule with a hole in it is the failure this pipeline already shipped.
    pub fn new(rest: BTreeMap<String, DQuat>, frames: Vec<Frame>) -> Result<Self> {
        ensure!(!rest.is_empty(), "a clip with no bone measures nothing");
        let (Some(first), Some(last)) = (frames.first(), frames.last()) else {
            anyhow::bail!("a clip with no frame measures nothing");
        };
        ensure!(
            first.seconds <= last.seconds,
            "the frames run from {} s to {} s, which is backwards",
            first.seconds,
            last.seconds
        );
        let start = first.seconds;
        let roles: BTreeSet<&str> = rest.keys().map(String::as_str).collect();
        for frame in &frames {
            ensure!(
                frame.seconds.is_finite(),
                "a frame sits at {} s, which is no time at all",
                frame.seconds
            );
            let held: BTreeSet<&str> = frame.rotations.keys().map(String::as_str).collect();
            ensure!(
                held == roles,
                "the frame at {} s carries {:?}, and the rest pose carries {:?}",
                frame.seconds,
                held,
                roles
            );
        }
        Ok(Self {
            rest,
            frames: frames
                .into_iter()
                .map(|frame| Frame {
                    seconds: frame.seconds - start,
                    ..frame
                })
                .collect(),
        })
    }

    /// The sidecar `retarget_animation.py` wrote beside its report.
    pub fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading the source motion {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("in {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let file: SourceMotionFile =
            serde_json::from_str(text).context("parsing the source motion")?;
        Self::new(
            quaternions(&file.rest, "the rest pose")?,
            file.frames
                .into_iter()
                .map(|frame| {
                    Ok(Frame {
                        seconds: frame.seconds,
                        rotations: quaternions(
                            &frame.rotations,
                            &format!("the frame at {} s", frame.seconds),
                        )?,
                    })
                })
                .collect::<Result<Vec<Frame>>>()?,
        )
    }

    /// Every role this clip drives, sorted.
    pub fn roles(&self) -> impl Iterator<Item = &str> {
        self.rest.keys().map(String::as_str)
    }

    /// One role's rest world rotation, on the rig this clip was authored on.
    pub fn rest(&self, role: &str) -> Option<DQuat> {
        self.rest.get(role).copied()
    }

    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }
}

/// Every row read as a unit rotation. A row that is not one is refused here
/// rather than normalized into a NaN three functions away.
fn quaternions(rows: &BTreeMap<String, [f64; 4]>, what: &str) -> Result<BTreeMap<String, DQuat>> {
    rows.iter()
        .map(|(role, [w, x, y, z])| {
            let rotation = DQuat::from_xyzw(*x, *y, *z, *w);
            ensure!(
                rotation.is_finite() && rotation.length() > 0.0,
                "{what} gives {role} the rotation {:?}, which is no rotation at all",
                [w, x, y, z]
            );
            Ok((role.clone(), rotation.normalize()))
        })
        .collect()
}
