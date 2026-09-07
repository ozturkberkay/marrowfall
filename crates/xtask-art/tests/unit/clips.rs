//! The synthetic cross-rig clip: one correct fit, and every way to break it.
//!
//! `clip.swing` and `clip.twist` need two rigs, and the real second rig is a
//! Mixamo FBX that this repository may not redistribute and that no Rust
//! reader opens. So the calibration pair is built here instead. It is
//! deterministic, it needs no download, there is no license to track, and
//! both expected values are hand-computable rather than measured.
//!
//! **What the vendor rig is.** Our own conformant rig with two differences
//! per bone, which are the two differences fact 3 and decision 6 measure
//! against Mixamo.
//!
//! - A **roll** about the bone's own +Y. The four leg bones a side carry
//!   [`LEG_ROLL_DEGREES`], which is fact 3's "171 to 175 degrees of pure roll
//!   at rest".
//! - A **tilt** about the bone's own +X, in the rest pose only, so the two
//!   rigs' bones point in different directions the way an A-pose and a T-pose
//!   do. The arms carry 62 degrees, which is what our rig and a Mixamo rig
//!   really measure apart.
//!
//! **Why both answers are zero.** Write the vendor's rest as
//! `S_rest = O_rest @ Ry(-roll) @ Rx(-tilt)` and its motion as
//! `S(t) = O(t) @ Ry(-roll)`, where `O` is our own output. Then
//!
//! - `S(t)^-1 @ O(t) = Ry(roll)`, whose swing is nothing, because a rotation
//!   about +Y cannot move +Y. So `clip.swing` is **0**.
//! - `S_rest^-1 @ O_rest = Rx(tilt) @ Ry(roll)`, which is already a swing
//!   times a twist about +Y, so its roll is `roll` and the frames' roll is
//!   `roll`. So `clip.twist` is **0**.
//!
//! What the rules actually read on the fixture is the `f32` the GLB stores,
//! and that is the calibration. The rest-direction residual a real aim table
//! adds is not synthetic and cannot be: it is 11.411 degrees, a hand
//! measurement on the three Mixamo clips, recorded in `[profile.clip]`.

use std::collections::BTreeMap;

use glam::{DMat3, DQuat, DVec3};
use xtask_art::check::gltf_world::{gltf_to_blender, gltf_to_blender_rotation};
use xtask_art::check::motion::{Frame, Motion};

use super::rigs::{Clip, Interpolation, SyntheticRig};

/// How far the vendor rig's legs are rolled from ours, in degrees. Fact 3
/// measures the real pair at 171 to 175.
pub const LEG_ROLL_DEGREES: f64 = 174.0;

/// The roles that stand on the floor, as `humanoid.toml` names them.
const GROUND_ROLES: [&str; 2] = ["left_toe", "right_toe"];

/// The role every other one hangs under, which carries a clip's travel.
const TOP: &str = "hips";

/// The two roles a step is sized across, as `humanoid.toml` names them.
const STRIDE_SEGMENT: [&str; 2] = ["left_upper_leg", "left_leg"];

/// How far the vendor clip's own root gets, in meters. `strafe_left`'s real
/// reading, and the sidecar declares it because no vendor rig is built here:
/// `clip.swing` and `clip.twist` need rotations and nothing else.
const SOURCE_TRAVEL_METERS: f64 = 2.3117;

/// And how far a standing pair travels instead, in meters.
///
/// A strafe crosses 2.04 m in a third of a second, which is 6 m/s of foot,
/// so a foot riding along with it never comes to rest. A pair that stands
/// still needs a travel the contact threshold can hold, and it cannot be zero
/// either: `clip.stride` has nothing to size a fit against a source that
/// never moved.
const STANDING_TRAVEL_METERS: f64 = 0.004;

/// The roles a standing pair holds at rest, so both feet keep the floor.
///
/// The hips as well as the legs: every joint below a turned hips is carried
/// by it, so a swinging root would take both feet off the ground with it.
const STANDING_ROLES: [&str; 9] = [
    "hips",
    "left_upper_leg",
    "left_leg",
    "left_foot",
    "left_toe",
    "right_upper_leg",
    "right_leg",
    "right_foot",
    "right_toe",
];

/// How much shorter our femur is than the vendor's, which is what every
/// length of a correct fit is sized by. The real Mixamo pair reads this.
pub const FEMUR_RATIO: f64 = 0.8815;

/// How many frames the fixture clip runs, and at what rate.
const FRAMES: usize = 8;
const FPS: f64 = 24.0;

/// Where the output's key times start, in frames.
///
/// A bought Mixamo clip runs frames 1 to 21 and a Meshy one starts at 0, so
/// the two sides here deliberately start one frame apart. Both rules align on
/// seconds from each clip's own start, so a correct pair still matches.
const OUTPUT_FIRST_FRAME: f64 = 1.0;

/// The vendor rig's roll and rest tilt per role, both in degrees, with the
/// side stripped off the name so one row serves left and right.
///
/// Every number is what the real pair measures, so the fixture is the shape
/// of the problem rather than a convenient one. The four leg rolls are fact
/// 3's 171 to 175 degrees. Each tilt is how far our A-posed rig's bone points
/// from a T-posed Mixamo rig's, read off `art/staging/downloads/strafe_left.fbx`
/// in Blender 5.2.1: 97.8 degrees on the hips is the sideways hip axis the
/// audit found, and 62 to 77 on the arms is the A-pose against the T-pose.
const DIFFERENCES: [(&str, f64, f64); 14] = [
    ("hips", 77.0, 97.8),
    ("spine_lower", 30.0, 9.8),
    ("spine_middle", 30.0, 9.8),
    ("spine_upper", 30.0, 7.4),
    ("neck", 30.0, 2.5),
    ("head", 30.0, 30.1),
    ("shoulder", 7.0, 7.4),
    ("arm", 70.0, 62.9),
    ("forearm", 55.4, 61.7),
    ("hand", 68.0, 77.2),
    ("upper_leg", LEG_ROLL_DEGREES, 5.5),
    ("leg", LEG_ROLL_DEGREES, 5.7),
    ("foot", 175.0, 14.4),
    ("toe", 173.0, 9.4),
];

/// One cross-rig pair: the GLB we deliver, and the sidecar the source arrives
/// in.
#[derive(Debug, Clone)]
pub struct CrossRig {
    bones: BTreeMap<String, String>,
    rig: SyntheticRig,
    /// Seconds against role to that bone's world rotation, in glTF Y-up.
    /// This is the correct fit, and the source is derived from it, so a
    /// defect is only ever injected on the output side.
    frames: Vec<(f64, BTreeMap<String, DQuat>)>,
    rest: BTreeMap<String, DQuat>,
    /// Role to an extra rotation the output carries in the bone's own frame.
    injected: BTreeMap<String, DQuat>,
    /// Writes the output one frame short of the source.
    output_short: bool,
    /// Multiplies every key time of the sidecar alone.
    source_rate: f64,
    /// Rotates the sidecar's rotations by one frame while its times stay put,
    /// which is what reading the sidecar for the wrong frame looks like.
    source_off_by_one: bool,
    /// Roles the sidecar leaves out.
    source_without: Vec<String>,
    /// Roles whose vendor bone rests pointing the other way, where the two
    /// bind poses have no roll between them to read.
    source_resting_opposite: Vec<String>,
    /// What the output's samplers declare.
    interpolation: Interpolation,
    /// Writes one key as a quaternion of no length at all.
    broken_key: bool,
    /// Scales the root's travel alone, which is what sizing a clip by the
    /// wrong femur leaves. The floor snap runs after the sizing, so the
    /// standing part of the same keys is left alone.
    travel_sized_by: f64,
    /// How far the vendor clip's own root gets, in meters.
    source_travel: f64,
    /// How far the whole clip is keyed under the floor it stands on.
    sunk: f64,
    /// How far the sidecar puts every source joint but the root from where
    /// its own rotations say the body is.
    source_displaced: DVec3,
}

impl CrossRig {
    /// A correct fit: every bone lands where the source's does, rolled by the
    /// constant the two rest poses call for.
    ///
    /// `bones` is the canonical convention out of `humanoid.toml`, so the
    /// fixture measures the real 22 roles.
    pub fn new(bones: BTreeMap<String, String>) -> Self {
        let rig = SyntheticRig::conformant();
        let rotations = rig.rest_rotations();
        let rest: BTreeMap<String, DQuat> = bones
            .iter()
            .filter_map(|(role, bone)| Some((role.clone(), *rotations.get(bone)?)))
            .collect();
        let frames = (0..FRAMES)
            .map(|frame| {
                let at = OUTPUT_FIRST_FRAME + frame as f64;
                let posed = rest
                    .iter()
                    .enumerate()
                    .map(|(index, (role, world))| (role.clone(), *world * pose(index, frame)))
                    .collect();
                (at / FPS, posed)
            })
            .collect();
        Self {
            bones,
            rig,
            frames,
            rest,
            injected: BTreeMap::new(),
            output_short: false,
            source_rate: 1.0,
            source_off_by_one: false,
            source_without: Vec::new(),
            source_resting_opposite: Vec::new(),
            interpolation: Interpolation::Linear,
            broken_key: false,
            travel_sized_by: 1.0,
            source_travel: SOURCE_TRAVEL_METERS,
            sunk: 0.0,
            source_displaced: DVec3::ZERO,
        }
    }

    /// Both feet flat on the floor for the whole clip, creeping forward.
    ///
    /// The hips and the eight leg bones are held at rest and the travel comes
    /// down to something a planted foot can hold, so the three foot contact
    /// rules have a pair that plants. Every other bone still moves.
    pub fn standing(mut self) -> Self {
        let rest = self.rest.clone();
        for (_, posed) in &mut self.frames {
            for role in STANDING_ROLES {
                if let Some(world) = rest.get(role) {
                    posed.insert(role.to_owned(), *world);
                }
            }
        }
        self.creeping(STANDING_TRAVEL_METERS)
    }

    /// How far the vendor clip's own root gets, in meters. Both sides move
    /// together, so a correct fit still reads nothing on `clip.stride`.
    pub fn creeping(mut self, meters: f64) -> Self {
        self.source_travel = meters;
        self
    }

    /// Moves every source joint but the root, leaving every rotation on both
    /// sides alone.
    ///
    /// The shape of the defect T15b shipped: our `Hips` carried a frame
    /// 97.612 degrees off the direction to its own `Spine`, so every bone
    /// still pointed exactly where the source's bone pointed and
    /// `clip.swing` read its own storage floor, while the torso hung 0.2150 m
    /// off the pelvis. `clip.posture` is the only rule that can see it.
    pub fn source_displaced_by(mut self, meters: f64) -> Self {
        self.source_displaced = DVec3::Z * meters;
        self
    }

    /// Keys the whole clip below the floor its own rig rests on, which is
    /// what puts a sole through the ground.
    pub fn sunk(mut self, meters: f64) -> Self {
        self.sunk = meters;
        self
    }

    /// The root's travel scaled, leaving the source's alone. `clip.stride`
    /// is the only rule that reads it, and 1.05 is a fit sized by a femur
    /// ratio five percent out.
    pub fn travel_sized_by(mut self, ratio: f64) -> Self {
        self.travel_sized_by = ratio;
        self
    }

    /// Our own stride segment at rest, off the fixture rig itself.
    pub fn femur(&self) -> f64 {
        let rest = self.rig.rest_positions();
        let at = |role: &str| rest[&self.bones[role]];
        (at(STRIDE_SEGMENT[1]) - at(STRIDE_SEGMENT[0])).length()
    }

    /// How far a correct fit of this pair travels: the vendor's own travel
    /// sized by the femur ratio, which is what `clip.stride` reads.
    pub fn travel(&self) -> f64 {
        self.source_travel * FEMUR_RATIO
    }

    /// Rolls one role's output bone about its own +Y, on every frame, leaving
    /// the source alone. This is the design's `q @ Quaternion((0, 1, 0),
    /// radians(90))`, which is a twist and not a yaw: a yaw would move where a
    /// downward thigh points and fire `clip.swing` instead.
    pub fn rolled(self, role: &str, degrees: f64) -> Self {
        self.turned(role, DQuat::from_rotation_y(degrees.to_radians()))
    }

    /// Swings one role's output bone about its own +X, on every frame, which
    /// moves where the bone points.
    pub fn swung(self, role: &str, degrees: f64) -> Self {
        self.turned(role, DQuat::from_rotation_x(degrees.to_radians()))
    }

    fn turned(mut self, role: &str, by: DQuat) -> Self {
        assert!(
            self.injected.insert(role.to_owned(), by).is_none(),
            "{role} already carries an injected rotation, and a second would \
             replace the first"
        );
        self
    }

    /// Repeats the first frame at the end, so the clip is a whole cycle.
    ///
    /// `pose` runs one full turn over [`FRAMES`], so the frame after the last
    /// is the first one again. `clip.loop` reads exactly that, and
    /// [`CrossRig::without_the_last_frame`] is then a clip cut one frame
    /// short of coming back around.
    pub fn looping(mut self) -> Self {
        let first = self.frames[0].1.clone();
        let at = OUTPUT_FIRST_FRAME + self.frames.len() as f64;
        self.frames.push((at / FPS, first));
        self
    }

    /// Writes the output one frame short of the source, which is a clip that
    /// lost a key on the way out.
    pub fn without_the_last_frame(mut self) -> Self {
        self.output_short = true;
        self
    }

    /// Reads the source at a different rate, which is what a 30 fps clip
    /// sampled in a 24 fps scene does: the two clips start together and drift
    /// apart frame by frame. A constant offset cannot serve, because both
    /// sides count from their own first frame on purpose.
    pub fn source_at_rate(mut self, factor: f64) -> Self {
        self.source_rate = factor;
        self
    }

    /// Gives every source frame the rotations of the frame after it, which is
    /// what reading the sidecar one row out looks like.
    pub fn source_off_by_one(mut self) -> Self {
        self.source_off_by_one = true;
        self
    }

    /// Leaves one role out of the source, which is a bone the vendor rig does
    /// not have.
    pub fn source_without(mut self, role: &str) -> Self {
        self.source_without.push(role.to_owned());
        self
    }

    /// Declares the output's samplers cubic, which stores two tangents beside
    /// every value and which this reader refuses rather than evaluates.
    pub fn with_cubic_keys(mut self) -> Self {
        self.interpolation = Interpolation::CubicSpline;
        self
    }

    /// Writes one rotation key with no length, which is no rotation at all.
    pub fn with_a_key_that_is_no_rotation(mut self) -> Self {
        self.broken_key = true;
        self
    }

    /// Rests one role's vendor bone pointing the other way, which is half a
    /// turn of swing and the one place a twist does not exist.
    pub fn source_resting_opposite(mut self, role: &str) -> Self {
        self.source_resting_opposite.push(role.to_owned());
        self
    }

    /// Renames one bone in the output, so the role it filled has nothing left
    /// to measure.
    pub fn output_without(mut self, role: &str) -> Self {
        let bone = self.bones[role].clone();
        self.rig = self.rig.renamed(&bone, &format!("not_{bone}"));
        self
    }

    /// Puts a translation channel on the armature object, which moves the
    /// whole clip while every node's own transform still matches the rig's.
    pub fn with_an_animated_object_node(mut self) -> Self {
        self.rig = self.rig.with_object_action();
        self
    }

    /// Takes the 0.01 scale off the armature node, which is exactly what
    /// `transform_apply(scale=True)` leaves behind: the rest geometry absorbs
    /// it and every location key keeps its bytes and changes its meaning.
    pub fn with_the_object_scale_applied(mut self) -> Self {
        self.rig = self.rig.scaled(1.0 / super::rigs::OBJECT_SCALE);
        self
    }

    /// The role above each role, for every role a bone still fills.
    ///
    /// A role whose bone this fixture renamed away has no row, so it gets no
    /// channel and the file simply has nothing filling it.
    fn parent_roles(&self) -> BTreeMap<String, Option<String>> {
        self.roles_above(&self.rig)
    }

    /// Every joint's world position at one frame, in Blender Z-up meters, on
    /// the pristine rig both sides of the pair share.
    ///
    /// **The fixture's own composition, not `gltf_clip`'s**, for the reason
    /// [`CrossRig::placed`] gives. The root's own lift and travel are left
    /// out: `clip.posture` reads every joint against the root, where both
    /// cancel, and the pristine rig is what keeps an output-side mutation
    /// from moving the source's joints with it.
    fn joints(&self, posed: &BTreeMap<String, DQuat>, injected: bool) -> BTreeMap<String, DVec3> {
        let parents = self.roles_above(&SyntheticRig::conformant());
        let places = SyntheticRig::conformant().rest_positions();
        let rest: BTreeMap<String, DVec3> = self
            .rest
            .keys()
            .filter_map(|role| Some((role.clone(), *places.get(&self.bones[role])?)))
            .collect();
        let carried: BTreeMap<String, DQuat> = posed
            .iter()
            .map(|(role, world)| {
                let turn = if injected {
                    self.injected.get(role).copied().unwrap_or(DQuat::IDENTITY)
                } else {
                    DQuat::IDENTITY
                };
                (role.clone(), *world * turn)
            })
            .collect();
        rest.keys()
            .map(|role| {
                (
                    role.clone(),
                    gltf_to_blender(self.placed(role, &carried, &rest, &parents)),
                )
            })
            .collect()
    }

    /// Role to the role above it, out of one rig's own bone hierarchy.
    fn roles_above(&self, rig: &SyntheticRig) -> BTreeMap<String, Option<String>> {
        let above: BTreeMap<String, Option<String>> = rig.hierarchy().into_iter().collect();
        self.bones
            .iter()
            .filter_map(|(role, bone)| {
                let parent = above.get(bone)?.clone().and_then(|parent| {
                    self.bones
                        .iter()
                        .find(|(_, other)| **other == parent)
                        .map(|(role, _)| role.clone())
                });
                Some((role.clone(), parent))
            })
            .collect()
    }

    /// Where one joint sits at one frame, composed up its own chain.
    ///
    /// **The fixture's own composition, not `gltf_clip`'s.** A fixture built
    /// with the reader under test cancels that reader out of every reading it
    /// takes. A rigid chain is enough here: each rest offset is carried by
    /// however far its parent turned from rest.
    fn placed(
        &self,
        role: &str,
        posed: &BTreeMap<String, DQuat>,
        rest: &BTreeMap<String, DVec3>,
        parents: &BTreeMap<String, Option<String>>,
    ) -> DVec3 {
        let Some(parent) = parents.get(role).and_then(Option::as_ref) else {
            return rest[role];
        };
        let turned = posed[parent] * self.rest[parent].inverse();
        self.placed(parent, posed, rest, parents) + turned * (rest[role] - rest[parent])
    }

    /// How far up this clip has to move for its lowest ground joint to stand
    /// where the rig itself rests, which is the lift the retarget applies.
    ///
    /// A correct fit stands on its own floor, so the fixture keys it: a
    /// rotation-only clip on this rig leaves its feet half a meter under the
    /// ground, and `clip.floor_snap` would rightly reject that.
    fn lift(&self) -> f64 {
        let parents = self.parent_roles();
        let bones = self.rig.rest_positions();
        let rest: BTreeMap<String, DVec3> = self
            .bones
            .iter()
            .filter_map(|(role, bone)| Some((role.clone(), *bones.get(bone)?)))
            .collect();
        let mut floor = f64::INFINITY;
        let mut lowest = f64::INFINITY;
        for role in GROUND_ROLES {
            floor = floor.min(rest[role].y);
            for (_, posed) in &self.frames {
                lowest = lowest.min(self.placed(role, posed, &rest, &parents).y);
            }
        }
        floor - lowest
    }

    /// The clip we deliver, as the bake would load it.
    pub fn output_glb(&self) -> Vec<u8> {
        let by_bone = |role: &str| self.bones[role].clone();
        let parent_role = self.parent_roles();
        let frames = &self.frames[..self.frames.len() - usize::from(self.output_short)];
        let carried = |role: &str, posed: &BTreeMap<String, DQuat>| {
            posed[role] * self.injected.get(role).copied().unwrap_or(DQuat::IDENTITY)
        };
        let mut clip = Clip {
            seconds: frames.iter().map(|(at, _)| *at).collect(),
            rotations: BTreeMap::new(),
            translations: BTreeMap::new(),
            interpolation: self.interpolation,
        };
        for role in parent_role.keys() {
            // The node stores its rotation against its parent, so the world
            // rotation the fixture chose is divided by the parent's.
            let mut keys: Vec<DQuat> = frames
                .iter()
                .map(|(_, posed)| match &parent_role[role] {
                    Some(parent) => carried(parent, posed).inverse() * carried(role, posed),
                    None => carried(role, posed),
                })
                .collect();
            if self.broken_key {
                keys[0] = DQuat::from_xyzw(0.0, 0.0, 0.0, 0.0);
            }
            clip.rotations.insert(by_bone(role), keys);
        }
        // The root's own location: where the clip stands, plus a straight
        // walk along +X spread over its keys. The object node above the
        // skeleton carries a 0.01 scale, so a local step is 100x its world
        // one.
        let root = by_bone(TOP);
        let standing = self.rig.rest_translation(&root)
            + DVec3::Y * ((self.lift() - self.sunk) / super::rigs::OBJECT_SCALE);
        let last = clip.seconds.len().saturating_sub(1).max(1) as f64;
        let step = DVec3::X * (self.travel() * self.travel_sized_by / super::rigs::OBJECT_SCALE);
        clip.translations.insert(
            root,
            (0..clip.seconds.len())
                .map(|key| standing + step * (key as f64 / last))
                .collect(),
        );
        self.rig.to_glb(&clip)
    }

    /// The same output, in `f64`, without ever going through a file.
    ///
    /// The pair against [`CrossRig::output_glb`] is what separates the two
    /// halves of the calibration: this one carries the math alone, and the
    /// file adds the `f32` a GLB stores.
    pub fn output_motion(&self) -> Motion {
        let blender = |rotations: &BTreeMap<String, DQuat>, injected: bool| {
            rotations
                .iter()
                .map(|(role, world)| {
                    let carried = if injected {
                        *world * self.injected.get(role).copied().unwrap_or(DQuat::IDENTITY)
                    } else {
                        *world
                    };
                    (role.clone(), gltf_to_blender_rotation(carried))
                })
                .collect()
        };
        let frames = &self.frames[..self.frames.len() - usize::from(self.output_short)];
        Motion::new(
            blender(&self.rest, false),
            frames
                .iter()
                .map(|(at, posed)| Frame {
                    seconds: *at,
                    rotations: blender(posed, true),
                    joints: self.joints(posed, true),
                })
                .collect(),
        )
        .expect("a well formed fixture motion")
    }

    /// The sidecar the source arrives in, as `clip.py` writes one.
    pub fn source_motion(&self) -> String {
        let rows = |rotations: &BTreeMap<String, DQuat>, tilted: bool| {
            rotations
                .iter()
                .filter(|(role, _)| !self.source_without.contains(role))
                .map(|(role, world)| {
                    let apart = if self.source_resting_opposite.contains(role) {
                        180.0
                    } else {
                        tilt(role)
                    };
                    let rest = if tilted {
                        DQuat::from_rotation_x(-apart.to_radians())
                    } else {
                        DQuat::IDENTITY
                    };
                    let vendor = *world * DQuat::from_rotation_y(-roll(role).to_radians()) * rest;
                    (role.clone(), quaternion(to_blender(vendor)))
                })
                .collect::<serde_json::Map<String, serde_json::Value>>()
        };
        // The vendor's body is ours sized by the femur ratio the sidecar
        // declares, so a correct fit reads nothing on `clip.posture` and what
        // is left is the `f32` the output GLB stores.
        let places = |posed: &BTreeMap<String, DQuat>| {
            self.joints(posed, false)
                .into_iter()
                .filter(|(role, _)| !self.source_without.contains(role))
                .map(|(role, at)| {
                    let bent = if role == TOP {
                        DVec3::ZERO
                    } else {
                        self.source_displaced
                    };
                    (role, point(at / FEMUR_RATIO + bent))
                })
                .collect::<serde_json::Map<String, serde_json::Value>>()
        };
        let last = self.frames.len() - 1;
        let frames: Vec<serde_json::Value> = self
            .frames
            .iter()
            .enumerate()
            .map(|(index, (_, posed))| {
                let read = if self.source_off_by_one {
                    &self.frames[(index + 1).min(last)].1
                } else {
                    posed
                };
                serde_json::json!({
                    "seconds": index as f64 / FPS * self.source_rate,
                    "rotations": rows(read, false),
                    "joints": places(read),
                })
            })
            .collect();
        serde_json::json!({
            "rest": rows(&self.rest, true),
            "frames": frames,
            "travel": self.source_travel,
            "stride_segment": self.femur() / FEMUR_RATIO,
        })
        .to_string()
    }
}

/// One bone's own pose at one frame: a swing about its +X and a roll about
/// its +Y, each a different phase per bone so no two bones move alike.
fn pose(bone: usize, frame: usize) -> DQuat {
    let phase = frame as f64 / FRAMES as f64 * std::f64::consts::TAU + bone as f64;
    DQuat::from_rotation_x((25.0 * phase.sin()).to_radians())
        * DQuat::from_rotation_y((15.0 * (2.0 * phase).sin()).to_radians())
}

/// How far the vendor rig rolls this role's bone from ours, in degrees.
fn roll(role: &str) -> f64 {
    difference(role).1
}

/// How far the vendor rig's bone points from ours at rest, in degrees.
fn tilt(role: &str) -> f64 {
    difference(role).2
}

/// The row that names this role, with the side stripped first so
/// `left_upper_leg` and `right_upper_leg` share one entry.
fn difference(role: &str) -> (&'static str, f64, f64) {
    let stem = role
        .strip_prefix("left_")
        .or_else(|| role.strip_prefix("right_"))
        .unwrap_or(role);
    *DIFFERENCES
        .iter()
        .find(|(named, _, _)| *named == stem)
        .unwrap_or_else(|| panic!("no cross-rig difference for the {role} role"))
}

/// The vendor's orientation in Blender Z-up world space, built from where the
/// vector conversion sends each of the bone's own axes.
///
/// **Not `gltf_to_blender_rotation`, on purpose.** The rules convert our
/// output with it, so a fixture that converted the source with it too would
/// cancel it out of every reading and any conversion at all would pass.
fn to_blender(rotation: DQuat) -> DQuat {
    let axis = |own: DVec3| gltf_to_blender(rotation * own);
    DQuat::from_mat3(&DMat3::from_cols(
        axis(DVec3::X),
        axis(DVec3::Y),
        axis(DVec3::Z),
    ))
}

/// A rotation as the sidecar writes one: `w, x, y, z`, Blender's own order.
fn point(at: DVec3) -> serde_json::Value {
    serde_json::json!([at.x, at.y, at.z])
}

fn quaternion(rotation: DQuat) -> serde_json::Value {
    serde_json::json!([rotation.w, rotation.x, rotation.y, rotation.z])
}
