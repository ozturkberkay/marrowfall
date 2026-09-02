//! Hand-built glTF rigs, so every rig rule has a positive fixture and a
//! negative one.
//!
//! The conformant rig is the positive. Each negative is that same rig with
//! one thing broken, which is what makes a failure name the rule rather than
//! the fixture.
//!
//! Every rig here carries the shape the real art carries: a 0.01 scale on the
//! object node above the skeleton, and joint translations 100x larger to
//! match. A reader that skips the node chain measures the rig at 170 m.

use glam::{DMat4, DQuat, DVec3};
use serde_json::{Value, json};

/// The scale the armature object carries in every file of this family.
pub const OBJECT_SCALE: f64 = 0.01;

/// What the conformant rig is built to, and what the profile asks for.
pub const HEIGHT_METERS: f64 = 1.70;
pub const HUMERUS_BELOW_HORIZONTAL: f64 = 40.0;

/// The object transform the local ones are derived against. Turning or
/// scaling a fixture moves the object node away from this, and that
/// difference is what the reader has to pick up.
fn base_object() -> DMat4 {
    DMat4::from_scale(DVec3::splat(OBJECT_SCALE))
}

/// One joint of a fixture: where it sits, and what it hangs from.
#[derive(Debug, Clone)]
struct Joint {
    /// What the fixture calls it. The hierarchy is held by this, so renaming
    /// a bone to one that is already taken leaves the tree intact.
    id: String,
    /// What the file calls it.
    name: String,
    /// `None` hangs the joint straight off the object node, outside the
    /// skeleton root.
    parent: Option<String>,
    /// World position, in meters, in glTF Y-up.
    position: DVec3,
    /// Multiplied into this joint's own local transform. Only a broken
    /// fixture uses it.
    local_extra: DMat4,
}

/// A rig under construction. Positions are world space, and the local
/// transforms are derived from them, the way an exporter derives them.
#[derive(Debug, Clone)]
pub struct SyntheticRig {
    /// Declaration order matters: a bone's own axis points at its first
    /// child, which is the child the profile names as its tail.
    joints: Vec<Joint>,
    /// The object node above the skeleton, as written.
    object: DMat4,
    object_action: bool,
}

impl SyntheticRig {
    /// A rig that breaks no rule: standard names, one root, every bone's +Y
    /// on its child, an exact mirror, arms at the angle the prompt asks for,
    /// 1.70 m tall, facing +Z in glTF.
    pub fn conformant() -> Self {
        let humerus = DVec3::new(
            HUMERUS_BELOW_HORIZONTAL.to_radians().cos(),
            -HUMERUS_BELOW_HORIZONTAL.to_radians().sin(),
            0.0,
        );
        let mut rig = Self {
            joints: Vec::new(),
            object: base_object(),
            object_action: false,
        };
        for (name, parent, position) in [
            ("Hips", None, DVec3::new(0.0, 0.95, 0.0)),
            ("Spine", Some("Hips"), DVec3::new(0.0, 1.06, 0.0)),
            ("Spine1", Some("Spine"), DVec3::new(0.0, 1.18, 0.0)),
            ("Spine2", Some("Spine1"), DVec3::new(0.0, 1.30, 0.0)),
            ("Neck", Some("Spine2"), DVec3::new(0.0, 1.42, 0.0)),
            ("Head", Some("Neck"), DVec3::new(0.0, 1.52, 0.0)),
            (
                "head_end",
                Some("Head"),
                DVec3::new(0.0, HEIGHT_METERS, 0.0),
            ),
            ("headfront", Some("Head"), DVec3::new(0.0, 1.52, 0.10)),
        ] {
            rig.push(name, parent, position);
        }
        for (side, out) in [("Left", 1.0), ("Right", -1.0)] {
            let mirror = DVec3::new(out, 1.0, 1.0);
            let arm = DVec3::new(0.17 * out, 1.40, 0.0);
            let fore_arm = arm + 0.26 * humerus * mirror;
            let hand = fore_arm + 0.24 * humerus * mirror;
            for (bone, parent, position) in [
                (
                    "Shoulder",
                    "Spine2".to_owned(),
                    DVec3::new(0.04 * out, 1.40, 0.0),
                ),
                ("Arm", format!("{side}Shoulder"), arm),
                ("ForeArm", format!("{side}Arm"), fore_arm),
                ("Hand", format!("{side}ForeArm"), hand),
                (
                    "UpLeg",
                    "Hips".to_owned(),
                    DVec3::new(0.09 * out, 0.90, 0.0),
                ),
                (
                    "Leg",
                    format!("{side}UpLeg"),
                    DVec3::new(0.09 * out, 0.50, 0.0),
                ),
                (
                    "Foot",
                    format!("{side}Leg"),
                    DVec3::new(0.09 * out, 0.08, 0.0),
                ),
                (
                    "ToeBase",
                    format!("{side}Foot"),
                    DVec3::new(0.09 * out, 0.0, 0.12),
                ),
            ] {
                rig.push(&format!("{side}{bone}"), Some(&parent), position);
            }
        }
        rig
    }

    fn push(&mut self, name: &str, parent: Option<&str>, position: DVec3) {
        self.joints.push(Joint {
            id: name.to_owned(),
            name: name.to_owned(),
            parent: parent.map(str::to_owned),
            position,
            local_extra: DMat4::IDENTITY,
        });
    }

    /// Turns the whole rig, from the object node. A yaw of 180 degrees faces
    /// it away from the camera, and a roll of 90 lays it on its side.
    pub fn turned(mut self, rotation: DQuat) -> Self {
        self.object = DMat4::from_quat(rotation) * self.object;
        self
    }

    /// Scales the whole rig, from the object node, which is where the 100x
    /// mistake lives in the real pipeline.
    pub fn scaled(mut self, factor: f64) -> Self {
        self.object *= DMat4::from_scale(DVec3::splat(factor));
        self
    }

    /// Hangs a bone somewhere else. `None` hangs it off the object node,
    /// outside the skeleton root.
    pub fn reparented(mut self, bone: &str, parent: Option<&str>) -> Self {
        self.at(bone).parent = parent.map(str::to_owned);
        self
    }

    /// Moves one joint on top of another, which leaves the segment between
    /// them with no direction at all.
    pub fn coincident(mut self, bone: &str, with: &str) -> Self {
        let position = self.at(with).position;
        self.at(bone).position = position;
        self
    }

    /// Collapses one bone's own axes, so it has no direction to measure.
    pub fn without_scale(mut self, bone: &str) -> Self {
        self.at(bone).local_extra = DMat4::from_scale(DVec3::ZERO);
        self
    }

    /// Moves one joint, in meters, in glTF Y-up.
    pub fn moved(mut self, bone: &str, by: DVec3) -> Self {
        self.at(bone).position += by;
        self
    }

    /// Renames one bone, the way an auto-rigger names one its own way. The
    /// name it takes can already be in use, which is its own defect.
    pub fn renamed(mut self, bone: &str, to: &str) -> Self {
        self.at(bone).name = to.to_owned();
        self
    }

    /// Moves one joint without letting its parent follow it, which is the
    /// only way a bone's own axis can end up off its child.
    pub fn nudged(mut self, bone: &str, by: DVec3) -> Self {
        self.at(bone).local_extra = DMat4::from_translation(by / OBJECT_SCALE);
        self
    }

    /// Puts an action on the armature object, which is the state that makes
    /// applying an object transform change the meaning of every key.
    pub fn with_object_action(mut self) -> Self {
        self.object_action = true;
        self
    }

    fn at(&mut self, bone: &str) -> &mut Joint {
        self.joints
            .iter_mut()
            .find(|joint| joint.id == bone)
            .unwrap_or_else(|| panic!("no {bone} in the fixture"))
    }

    fn index_of(&self, bone: Option<&str>) -> Option<usize> {
        let bone = bone?;
        self.joints.iter().position(|joint| joint.id == bone)
    }

    /// The joint's own +Y, aimed at its first child. A leaf drives nothing,
    /// so it keeps whatever its parent had.
    fn rotation(&self, index: usize) -> DQuat {
        let joint = &self.joints[index];
        match self
            .joints
            .iter()
            .find(|other| other.parent.as_deref() == Some(joint.id.as_str()))
        {
            Some(child) => DQuat::from_rotation_arc(
                DVec3::Y,
                (child.position - joint.position)
                    .try_normalize()
                    .unwrap_or(DVec3::Y),
            ),
            None => self
                .index_of(joint.parent.as_deref())
                .map_or(DQuat::IDENTITY, |parent| self.rotation(parent)),
        }
    }

    /// The world transform the fixture asks for, object scale included, so a
    /// local translation reads 100x its world one.
    fn world(&self, index: usize) -> DMat4 {
        DMat4::from_translation(self.joints[index].position)
            * DMat4::from_quat(self.rotation(index))
            * base_object()
    }

    /// The rig as a glTF document. Local transforms are derived from the
    /// world ones, which is the direction an exporter works in, and against
    /// the base object transform, so turning or scaling the object node
    /// really moves the rig.
    pub fn to_gltf(&self) -> String {
        let mut nodes = vec![json!({
            "name": "Armature",
            "matrix": columns(self.object),
            "children": self.children(None),
        })];
        for (index, joint) in self.joints.iter().enumerate() {
            let parent = self
                .index_of(joint.parent.as_deref())
                .map_or_else(base_object, |parent| self.world(parent));
            let local = parent.inverse() * self.world(index) * joint.local_extra;
            let mut node = json!({ "name": joint.name, "matrix": columns(local) });
            let children = self.children(Some(&joint.id));
            if !children.is_empty() {
                node["children"] = json!(children);
            }
            nodes.push(node);
        }
        let mut document = json!({
            "asset": { "version": "2.0", "generator": "marrowfall test fixture" },
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": nodes,
            "skins": [{
                "name": "Armature",
                "joints": (1..=self.joints.len()).collect::<Vec<usize>>(),
            }],
        });
        if self.object_action {
            document["animations"] = object_action();
            document["accessors"] = one_key_accessors();
            document["bufferViews"] = one_key_buffer_views();
            document["buffers"] = json!([{
                "byteLength": 16,
                "uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAAAAAA==",
            }]);
        }
        document.to_string()
    }

    /// The node indices hanging off one bone, or off the object node. Node 0
    /// is the object, so a joint's node index is its own plus one.
    fn children(&self, parent: Option<&str>) -> Vec<usize> {
        self.joints
            .iter()
            .enumerate()
            .filter(|(_, joint)| joint.parent.as_deref() == parent)
            .map(|(index, _)| index + 1)
            .collect()
    }
}

/// A matrix as glTF writes one: sixteen floats, column major.
fn columns(matrix: DMat4) -> Vec<f64> {
    matrix.to_cols_array().to_vec()
}

/// One channel, on the object node, which is node 0.
fn object_action() -> Value {
    json!([{
        "name": "on the object",
        "channels": [{ "sampler": 0, "target": { "node": 0, "path": "translation" } }],
        "samplers": [{ "input": 0, "output": 1, "interpolation": "LINEAR" }],
    }])
}

fn one_key_accessors() -> Value {
    json!([
        {
            "bufferView": 0, "componentType": 5126, "count": 1, "type": "SCALAR",
            "min": [0.0], "max": [0.0],
        },
        { "bufferView": 1, "componentType": 5126, "count": 1, "type": "VEC3" },
    ])
}

fn one_key_buffer_views() -> Value {
    json!([
        { "buffer": 0, "byteOffset": 0, "byteLength": 4 },
        { "buffer": 0, "byteOffset": 4, "byteLength": 12 },
    ])
}
