//! The lock file: machine-owned record of what has already been done, so a
//! re-run skips completed work instead of re-spending credits. A sidecar, to
//! keep the hand-authored spec diffable.
//!
//! Stages are fingerprinted: changing a spec field a stage depends on
//! invalidates it and everything downstream, so a stale mesh is never paired
//! with new settings.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::library::{AnimationLibrary, MotionSource};
use crate::meshy::Endpoint;
use crate::spec::CharacterSpec;

/// One step of the pipeline. Ordering is the execution order, and
/// [`Stage::all`] is the canonical sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Stage {
    /// Generate the four concept views (OpenAI).
    Concept,
    /// Reconstruct a 3D mesh from those views, then remesh and texture it (Meshy).
    Model,
    /// Auto-rig and attach animations (Meshy). Skipped for unriggable bodies.
    Rig,
    /// Fetch the finished GLB. This is the checkpoint everything downstream rebuilds from.
    Download,
    /// Render sprite frames from the GLB (Blender, local).
    Bake,
    /// Trim, anchor and pack frames into atlases, and write the manifest the
    /// game reads. One stage, because the manifest describes that layout.
    Pack,
}

/// Who gets billed for a stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    OpenAI,
    Meshy,
}

impl Stage {
    pub const fn all() -> [Stage; 6] {
        [
            Stage::Concept,
            Stage::Model,
            Stage::Rig,
            Stage::Download,
            Stage::Bake,
            Stage::Pack,
        ]
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Stage::Concept => "concept",
            Stage::Model => "model",
            Stage::Rig => "rig",
            Stage::Download => "download",
            Stage::Bake => "bake",
            Stage::Pack => "pack",
        }
    }

    /// Which provider this stage spends money with, if any. Concept bills
    /// OpenAI, the model stages bill Meshy.
    pub const fn provider(self) -> Option<Provider> {
        match self {
            Stage::Concept => Some(Provider::OpenAI),
            Stage::Model | Stage::Rig => Some(Provider::Meshy),
            Stage::Download | Stage::Bake | Stage::Pack => None,
        }
    }

    /// Whether this stage spends money. Used to decide what to warn about
    /// before re-running.
    pub const fn costs_credits(self) -> bool {
        self.provider().is_some()
    }

    /// Stages that run after this one, in order.
    pub fn downstream(self) -> Vec<Stage> {
        Stage::all().into_iter().filter(|&s| s > self).collect()
    }
}

impl std::str::FromStr for Stage {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        Stage::all()
            .into_iter()
            .find(|stage| stage.as_str() == s)
            .with_context(|| {
                let names: Vec<_> = Stage::all().iter().map(|s| s.as_str()).collect();
                format!("unknown stage {s:?}, expected one of: {}", names.join(", "))
            })
    }
}

impl std::fmt::Display for Stage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A remote task, recorded so a later stage can fetch its result. Each
/// variant carries the spec inputs that produced it, so a resumed run only
/// reuses a task whose inputs still match.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TaskRef {
    /// The generated mesh.
    Model { id: String },
    /// The rigged character.
    Rig { id: String, height_meters: f32 },
    /// One animation attached to a rig.
    Animation {
        id: String,
        name: String,
        action_id: u32,
    },
}

impl TaskRef {
    pub fn id(&self) -> &str {
        match self {
            Self::Model { id } | Self::Rig { id, .. } | Self::Animation { id, .. } => id,
        }
    }

    /// The endpoint this task's result must be polled from.
    pub fn endpoint(&self) -> Endpoint {
        match self {
            Self::Model { .. } => Endpoint::MultiImageTo3d,
            Self::Rig { .. } => Endpoint::Rigging,
            Self::Animation { .. } => Endpoint::Animation,
        }
    }
}

/// Record of one completed stage.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StageRecord {
    /// Fingerprint of the spec fields this stage consumed. A mismatch means
    /// the inputs changed and the stage must run again.
    pub fingerprint: String,
    /// Remote tasks this stage created, for provenance and for downstream
    /// stages to fetch results from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<TaskRef>,
    /// Credits spent, when the provider reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credits: Option<u32>,
    /// Free-form notes, e.g. which animations were baked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Persistent progress for one character.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Lock {
    #[serde(default)]
    pub stages: BTreeMap<Stage, StageRecord>,
}

impl Lock {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading lock {}", path.display()))?;
        ron::from_str(&text)
            .with_context(|| format!("parsing lock {} (delete it to start over)", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let config = ron::ser::PrettyConfig::new().struct_names(true);
        // ron omits the trailing newline; without it every write trips the
        // end-of-file pre-commit hook.
        let text = ron::ser::to_string_pretty(self, config)? + "\n";
        std::fs::write(path, text).with_context(|| format!("writing lock {}", path.display()))
    }

    /// Whether `stage` has completed with inputs matching the current spec.
    pub fn is_current(
        &self,
        stage: Stage,
        spec: &CharacterSpec,
        library: &AnimationLibrary,
    ) -> bool {
        self.stages
            .get(&stage)
            .is_some_and(|record| record.fingerprint == fingerprint(stage, spec, library))
    }

    /// Marks a stage complete and invalidates every stage after it. A new
    /// mesh makes old sprites wrong even when their fingerprints still match,
    /// since a fingerprint covers spec fields, not upstream artefacts.
    pub fn record(
        &mut self,
        stage: Stage,
        spec: &CharacterSpec,
        library: &AnimationLibrary,
        record: StageRecord,
    ) {
        self.stages.insert(
            stage,
            StageRecord {
                fingerprint: fingerprint(stage, spec, library),
                ..record
            },
        );
        for downstream in stage.downstream() {
            self.stages.remove(&downstream);
        }
    }

    /// Tasks recorded by any stage, newest stage last. Used to resume a
    /// partially completed rig without re-paying for finished sub-tasks.
    pub fn tasks(&self) -> Vec<TaskRef> {
        self.stages
            .values()
            .flat_map(|record| record.tasks.iter().cloned())
            .collect()
    }
}

/// Fingerprints exactly the spec fields a stage consumes. Narrow on purpose:
/// editing a sprite setting must not invalidate a paid stage.
pub fn fingerprint(stage: Stage, spec: &CharacterSpec, library: &AnimationLibrary) -> String {
    let mut parts = vec![format!("{:?}", spec.subject.kind)];
    // Bump when the bake or packing algorithm changes in a way that makes
    // existing output wrong. A fingerprint over spec fields alone cannot
    // express "the code that produced this has been fixed", so without this
    // a corrected packer would report `cached` forever.
    if matches!(stage, Stage::Bake | Stage::Pack) {
        parts.push(format!("algo{LOCAL_PIPELINE_VERSION}"));
    }
    match stage {
        Stage::Concept => {
            parts.push(spec.subject.description.clone());
            // The pose is prompt text, not a spec field, so editing it in code
            // has to invalidate the concept the same way editing the
            // description does.
            parts.push(spec.subject.kind.pose_instruction().to_owned());
        }
        Stage::Model => {
            parts.push(format!("{:?}", spec.remesh));
            parts.push(format!("{:?}", spec.texture));
        }
        Stage::Rig | Stage::Download => {
            // Only the animations requested, not what we call them: renaming a animation
            // locally must not trigger a re-rig, which is several charges.
            parts.push(format!("{}", spec.subject.height_meters));
            // The action ids, not the names: renaming an animation in the
            // library must not trigger a re-rig, which is several charges.
            // A name that no longer resolves contributes nothing here — the
            // stage itself reports that far more clearly.
            let mut ids: Vec<u32> = spec
                .animations
                .iter()
                .filter_map(|name| library.animations.get(name))
                .filter_map(|animation| match animation.source {
                    // Authored motion costs nothing and cannot go stale, so it
                    // has no bearing on whether the paid rig must run again.
                    MotionSource::Meshy { action_id } => Some(action_id),
                    MotionSource::Authored => None,
                })
                .collect();
            ids.sort_unstable();
            parts.push(format!("{ids:?}"));
        }
        Stage::Bake => {
            // `sprite_height` is deliberately excluded — it is Pack's input,
            // and re-rendering hundreds of frames to change a downscale
            // target would be pure waste.
            parts.push(format!(
                "{}/{}/{}/{}/{}",
                spec.bake.directions,
                spec.bake.render_size,
                spec.bake.fps,
                spec.bake.forearm_roll,
                spec.bake.trim_start
            ));
            // The bake reads one file per animation, keyed on the library's
            // name — not the action id, which only the paid stages use.
            parts.extend(spec.animations.iter().cloned());
        }
        Stage::Pack => {
            parts.push(spec.name.clone());
            parts.push(format!(
                "{}/{}",
                spec.bake.sprite_height, spec.bake.directions
            ));
            for name in &spec.animations {
                let loops = library.animations.get(name).is_some_and(|a| a.loops);
                parts.push(format!("{name}:{loops}"));
            }
        }
    }
    // A digest would be shorter, but a readable fingerprint makes lock diffs
    // explain themselves when a stage unexpectedly re-runs.
    let joined = parts.join("|");
    format!("{:016x}", fnv1a(joined.as_bytes()))
}

/// Version of the free half of the pipeline (bake and pack). Bump whenever
/// its output changes for identical inputs.
/// 2: shared camera framing and ground line, root-motion travel stripped.
pub const LOCAL_PIPELINE_VERSION: u32 = 2;

/// FNV-1a. Not cryptographic — this only needs to detect edits, and avoiding a
/// hashing dependency keeps the tool's dependency surface small.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}
