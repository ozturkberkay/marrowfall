//! The lock file: machine-owned record of what has already been done, so a
//! re-run skips completed work instead of re-spending credits. A sidecar, to
//! keep the hand-authored spec diffable.
//!
//! Stages are fingerprinted over the inputs they actually read: the spec
//! fields, and the *content* of the art files. Changing one invalidates that
//! stage and everything downstream, so a stale mesh is never paired with new
//! settings and a replaced rig cannot report `cached`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::blender::{BLENDER_SRC, Build};
use crate::check::profile::Profile;
use crate::library::{AnimationLibrary, MotionSource};
use crate::providers::meshy::Endpoint;
use crate::spec::{Bake, CharacterSpec, Paths, Subject, View};

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
    /// Fetch the finished GLBs, rename and conform the rig, and fit each
    /// bought clip onto it. `model.glb` is the checkpoint everything
    /// downstream rebuilds from.
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

impl Provider {
    /// What this provider charges for, so nothing recommends a re-run
    /// without naming whose bill it is.
    pub const fn bills(self) -> &'static str {
        match self {
            Provider::OpenAI => "OpenAI images",
            Provider::Meshy => "Meshy credits",
        }
    }
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

    /// Whether [`LOCAL_PIPELINE_VERSION`] rides in this stage's fingerprint.
    ///
    /// The four that run code of ours: the model stage downloads the bare
    /// mesh every gate and the fixer read, the download stage renames and
    /// conforms the rig it fetched and fits every bought clip onto it, and
    /// the bake and the pack produce every frame and every atlas. Bumping the
    /// version re-runs all four, and the model stage spends Meshy credits when
    /// it does. The download stage spends nothing: it keeps the vendor's own
    /// files and re-runs the local work on them.
    pub const fn is_versioned(self) -> bool {
        matches!(
            self,
            Stage::Model | Stage::Download | Stage::Bake | Stage::Pack
        )
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
    /// Fingerprint of what this stage read: its spec fields, and the content
    /// of the art files. A mismatch means an input changed and the stage must
    /// run again.
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

    /// Whether `stage` has completed with inputs matching what is on disk now.
    pub fn is_current(&self, stage: Stage, inputs: &Inputs<'_>) -> Result<bool> {
        let Some(record) = self.stages.get(&stage) else {
            return Ok(false);
        };
        Ok(record.fingerprint == fingerprint(stage, inputs)?)
    }

    /// What every stage reads as, in pipeline order.
    ///
    /// Never fails: an input that cannot be read leaves one stage
    /// [`State::Unknown`] and the rest answerable, because the command that
    /// prints this has to print something on any machine.
    pub fn states(&self, inputs: &Inputs<'_>) -> BTreeMap<Stage, State> {
        Stage::all()
            .into_iter()
            .map(|stage| {
                let state = match self.stages.get(&stage) {
                    None => State::Todo,
                    Some(record) => match fingerprint(stage, inputs) {
                        Ok(current) if current == record.fingerprint => State::Done,
                        Ok(_) => State::Stale,
                        Err(error) => State::Unknown(format!("{error}")),
                    },
                };
                (stage, state)
            })
            .collect()
    }

    /// Marks a stage complete and invalidates every stage after it. A new
    /// mesh makes old sprites wrong even when their fingerprints still match,
    /// since no downstream stage reads every upstream file.
    pub fn record(&mut self, stage: Stage, inputs: &Inputs<'_>, record: StageRecord) -> Result<()> {
        self.stages.insert(
            stage,
            StageRecord {
                fingerprint: fingerprint(stage, inputs)?,
                ..record
            },
        );
        for downstream in stage.downstream() {
            self.stages.remove(&downstream);
        }
        Ok(())
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

/// What the lock says about one stage, against the inputs on disk now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Nothing recorded it.
    Todo,
    /// Recorded, and every input still matches.
    Done,
    /// Recorded under inputs that have since moved.
    Stale,
    /// Recorded, and an input could not be read: this is what stopped it.
    Unknown(String),
}

impl State {
    /// The one word a table prints.
    pub const fn word(&self) -> &'static str {
        match self {
            State::Todo => "todo",
            State::Done => "done",
            State::Stale => "stale",
            State::Unknown(_) => "unknown",
        }
    }

    /// What stopped it, when that is the answer.
    pub fn reason(&self) -> Option<&str> {
        match self {
            State::Unknown(why) => Some(why),
            _ => None,
        }
    }
}

/// The stale entries of [`Lock::states`], in pipeline order. An
/// [`State::Unknown`] stage is not among them: nothing can say it moved.
pub fn stale(states: &BTreeMap<Stage, State>) -> Vec<Stage> {
    states
        .iter()
        .filter(|(_, state)| **state == State::Stale)
        .map(|(stage, _)| *stage)
        .collect()
}

/// Everything a fingerprint reads, gathered once per command.
#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    pub root: &'a Path,
    pub spec: &'a CharacterSpec,
    pub library: &'a AnimationLibrary,
    /// Asked for its build only by the stage that renders, so a command that
    /// touches no other stage needs no Blender.
    pub blender: &'a Build,
}

/// Fingerprints exactly the inputs a stage consumes: its spec fields, and the
/// content of the art files it reads. Narrow on purpose: editing a sprite
/// setting must not invalidate a paid stage, and renaming a clip locally must
/// not either. `Pack` is the one stage whose files are not hashed, and its
/// arm says why.
pub fn fingerprint(stage: Stage, inputs: &Inputs<'_>) -> Result<String> {
    let Inputs {
        root,
        spec,
        library,
        blender,
    } = inputs;
    // The spec is destructured all the way down rather than read field by
    // field: adding a field is then `E0027` here, and claiming it in no arm
    // below is an unused binding, which `-D warnings` turns into a decision.
    // Digesting a struct whole needs no code change and would put the
    // description into the paid rig and `sprite_height` into the bake.
    let CharacterSpec {
        name,
        subject,
        animations,
        remesh,
        texture,
        bake,
    } = *spec;
    let Subject {
        kind,
        description,
        height_meters,
        skeleton,
        cleanup,
        symmetry,
        pose_mode,
    } = subject;
    let Bake {
        directions,
        render_size,
        sprite_height,
        trim_start,
    } = bake;
    let paths = Paths::new(root, name);
    let mut parts = vec![format!("{kind:?}")];
    // Bump when a stage's own code changes in a way that makes existing
    // output wrong. A fingerprint over inputs alone cannot express "the code
    // that produced this has been fixed", so without this a corrected packer
    // would report `cached` forever.
    if stage.is_versioned() {
        parts.push(format!("algo{LOCAL_PIPELINE_VERSION}"));
    }
    match stage {
        Stage::Concept => {
            parts.push(description.clone());
            // The pose is prompt text, not a spec field, so editing it in code
            // has to invalidate the concept the same way editing the
            // description does.
            parts.push(kind.pose_instruction().to_owned());
        }
        Stage::Model => {
            // Whole-struct, because only this stage reads either one, so a new
            // remesh or texture setting needs no code change here.
            parts.push(format!("{remesh:?}"));
            parts.push(format!("{texture:?}"));
            // The rest pose the request asks for, which is a different body.
            parts.push(format!("{pose_mode:?}"));
            // The mesh is reconstructed from the four views, so a regenerated
            // view is a different mesh.
            for view in View::ALL {
                parts.push(derived_file(&paths.concept(view))?);
            }
        }
        Stage::Rig | Stage::Download => {
            parts.push(format!("{height_meters}/{skeleton}/{cleanup}/{symmetry}"));
            // The download stage renames and conforms what it fetched and
            // fits every bought clip onto the result, so it reads the
            // vendor's file plus everything a fit reads: the canonical rig,
            // the profile, the Blender build and the scripts. The rig stage
            // reads none of it: it is the paid one, and editing a
            // `[profile.tails]` row must not bill.
            if stage == Stage::Download {
                parts.push(derived_file(&paths.rigged_glb())?);
                parts.push(blender_inputs(root, skeleton, blender.read()?)?);
            }
            // The action ids, not the names: renaming an animation in the
            // library must not trigger a re-rig, which is several charges.
            // A name that no longer resolves contributes nothing here, the
            // stage itself reports that far more clearly.
            let mut ids: Vec<u32> = animations
                .iter()
                .filter_map(|name| library.animations.get(name))
                .filter_map(|animation| match animation.source {
                    // Only Meshy ids decide whether a paid rig has to run
                    // again. Free motion cannot invalidate a paid stage.
                    MotionSource::Meshy { action_id } => Some(action_id),
                    MotionSource::Mixamo { .. } | MotionSource::Authored => None,
                })
                .collect();
            ids.sort_unstable();
            parts.push(format!("{ids:?}"));
            // The mesh sent to rigging, and the download the fixer made it
            // from. Both are derived and gitignored, so on a machine that
            // does not hold them this reads absent and the stage reports
            // stale; the checkpoint GLB is what stops that spending anything.
            parts.push(derived_file(&paths.bare_glb())?);
            parts.push(derived_file(&paths.clean_glb())?);
        }
        Stage::Bake => {
            // `sprite_height` is deliberately excluded, it is Pack's input,
            // and re-rendering hundreds of frames to change a downscale
            // target would be pure waste.
            parts.push(format!("{directions}/{render_size}/{trim_start}"));
            parts.push(blender_inputs(root, skeleton, blender.read()?)?);
            // The character it renders. The download stage writes it, so a
            // character that has not reached that stage reads absent.
            parts.push(derived_file(&paths.character_glb())?);
            // The bake reads one file per animation, keyed on the library's
            // name, not the action id, which only the paid stages use. The
            // rate rides along because it decides the frame count, and the
            // file itself because a refetched clip is different motion under
            // the same name.
            for clip in animations {
                parts.push(
                    library
                        .get(clip)
                        .map_or_else(|_| clip.clone(), |a| format!("{clip}@{}", a.fps)),
                );
                parts.push(derived_file(&library.glb(root, clip))?);
            }
        }
        Stage::Pack => {
            // Hashes no file, alone among the six. It reads the staging PNGs,
            // and every one of them was written by the bake this record
            // already sits downstream of.
            parts.push(name.clone());
            parts.push(format!("{sprite_height}/{directions}"));
            for clip in animations {
                let loops = library.animations.get(clip).is_some_and(|a| a.loops);
                parts.push(format!("{clip}:{loops}"));
            }
        }
    }
    Ok(digest(parts.join("|").as_bytes()))
}

/// What every Blender stage reads and no spec field names: the skeleton's
/// canonical rig and its profile, the Blender build, and every script.
///
/// One function, so an `[aim_table]` row invalidates the retarget and the bake
/// together. The rig, the profile and the scripts are committed, so a missing
/// one is an error here rather than a hash of nothing.
pub fn blender_inputs(root: &Path, skeleton: &str, blender: &str) -> Result<String> {
    let parts = [
        committed_file(&AnimationLibrary::reference_rig(root, skeleton))?,
        committed_file(&Profile::path(root, skeleton))?,
        blender.to_owned(),
        scripts(root)?,
    ];
    Ok(digest(parts.join("|").as_bytes()))
}

/// One digest over every Blender script, so a fix to any of them re-runs the
/// stages that shell out to Blender. Named, so moving code between two of
/// them still reads as a change.
fn scripts(root: &Path) -> Result<String> {
    let dir = root.join(BLENDER_SRC);
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .with_context(|| format!("reading the Blender scripts in {}", dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "py"))
        .collect();
    files.sort();
    anyhow::ensure!(!files.is_empty(), "no Blender scripts in {}", dir.display());
    let mut parts = Vec::new();
    for path in files {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        parts.push(format!("{name}:{}", committed_file(&path)?));
    }
    Ok(digest(parts.join("|").as_bytes()))
}

/// The content of a file a stage cannot run without.
///
/// Missing is an error rather than an empty hash, which every later run would
/// read as "nothing changed".
fn committed_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| {
        format!(
            "fingerprinting {}: this file is committed, so a checkout has it",
            path.display()
        )
    })?;
    Ok(digest(&bytes))
}

/// The content of a file the pipeline itself produces, or an explicit
/// absence when the stage that writes it has not run yet.
///
/// Content, never a date: a fresh checkout has new timestamps and the same
/// art.
fn derived_file(path: &Path) -> Result<String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(digest(&bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok("absent".to_owned()),
        Err(error) => Err(error).with_context(|| format!("fingerprinting {}", path.display())),
    }
}

/// The hex fingerprint every lock file in this crate records. Shared so the
/// character lock and the library lock cannot drift apart.
pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:016x}", fnv1a(bytes))
}

/// Version of the code the model, bake and pack stages run. Bump whenever
/// their output changes for identical inputs, and know that re-running the
/// model stage spends Meshy credits.
///
/// 2: shared camera framing and ground line, root-motion travel stripped.
/// 3: clip translation sized to the character playing it.
pub const LOCAL_PIPELINE_VERSION: u32 = 3;

/// FNV-1a. Not cryptographic, this only needs to detect edits, and avoiding a
/// hashing dependency keeps the tool's dependency surface small.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}
