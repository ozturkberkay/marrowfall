//! The shared animation library: motion declared once, reused by every
//! character on the same skeleton, the way Godot and Unity share clips.
//!
//! Sharing works in two halves. Motion is fitted to the skeleton's canonical
//! rig when it is fetched, so every clip has the same bone names and the same
//! rest pose; and its `location` channels, which are lengths rather than
//! proportion-independent rotations, are sized to whichever character is
//! playing it when it is baked.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::check::{Artifacts, Report, Severity};

/// This project's standard biped: 24 bones, no fingers, named by the
/// canonical `standard` convention. A hand-rigged one must match
/// `art/skeletons/humanoid.toml` to be the same skeleton.
pub const HUMANOID: &str = "humanoid";

/// Where a motion comes from, and so how its file gets on disk. Nothing
/// downstream asks; adding a provider is a new variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MotionSource {
    /// Retargeted from Meshy's animation library, selected by numeric id.
    /// Bought once, then shared by every character on the same skeleton.
    Meshy { action_id: u32 },
    /// Fetched from Mixamo by product id, then fitted to the canonical rig.
    /// Free, so `costs_credits` stays false.
    Mixamo { product_id: String },
    /// Hand-authored, in Blender, or anywhere else, and committed with the
    /// rest of the art. There is nothing to fetch and nothing to pay for.
    Authored,
}

impl MotionSource {
    /// Whether obtaining this motion costs money.
    pub const fn costs_credits(&self) -> bool {
        matches!(self, Self::Meshy { .. })
    }

    /// Whether the file itself may be published with the rest of the art.
    ///
    /// Meshy's paid plan hands ownership over, and those clips cost credits,
    /// so committing them stops every contributor buying them again. Adobe
    /// grants the *use* of a Mixamo animation but forbids redistributing the
    /// file, and this repository is public. One boolean per provider, so
    /// adding one is a decision rather than a license audit.
    pub const fn redistributable(&self) -> bool {
        match self {
            Self::Meshy { .. } | Self::Authored => true,
            Self::Mixamo { .. } => false,
        }
    }

    /// How the file this motion arrives in names its bones, a table in
    /// `art/skeletons/<skeleton>.toml`. Declared rather than guessed from the
    /// file, so a new provider is a decision the compiler asks for.
    ///
    /// What the vendor ships, which is what the source check and the retarget
    /// open: Meshy animates the rig it sold and delivers it under its own
    /// names, Mixamo delivers its own FBX, and authored motion is made on our
    /// rig. The copy this repository commits under `art/animations/` is in the
    /// standard convention whoever sold the motion, and `[fingerprints]` is
    /// what says so about a file in hand.
    pub const fn bone_convention(&self) -> &'static str {
        match self {
            Self::Meshy { .. } => "meshy",
            Self::Mixamo { .. } => "mixamo",
            Self::Authored => "standard",
        }
    }
}

/// One motion, described by what it is rather than who uses it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Animation {
    /// The skeleton this motion drives. F-curves address bones by name, so a
    /// humanoid walk cannot drive an insectoid. A name, not an enum, because
    /// hand-rigging a creature invents a new one.
    pub skeleton: String,
    /// Whether playback repeats. Locomotion loops; a death does not.
    pub loops: bool,
    /// Sprite frames sampled per second. A property of the motion, not of any
    /// character: an idle barely changes between frames while a run changes a
    /// third of its silhouette, so one shared rate is simultaneously too fast
    /// for one and too slow for the other.
    pub fps: u32,
    /// The rate the clip itself was authored at, which is not [`Self::fps`].
    /// glTF stores key times in seconds, so reading a 30 fps clip in a 24 fps
    /// scene lands its frames at 0.8 to 16.8 and rounding drops four of them.
    /// The retarget sets the scene rate from this, and `clip.fps_grid` is the
    /// rule that says every key landed whole.
    pub source_fps: u32,
    /// Whether the clip's hips end up somewhere other than where they
    /// started. `source.traveling` reads exactly this against the vendor
    /// file, so the declaration and the measurement cannot mean two
    /// different things.
    pub travels: bool,
    pub source: MotionSource,
}

/// Every animation available to every character.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnimationLibrary {
    pub animations: BTreeMap<String, Animation>,
}

impl AnimationLibrary {
    /// Where the library is declared, relative to the repo root.
    pub fn path(root: &Path) -> PathBuf {
        root.join("art/animations/library.ron")
    }

    /// The GLB holding one animation's motion, with no mesh.
    ///
    /// Motion we may publish sits beside the library and is committed; motion
    /// we may not is gitignored and re-fetched on each machine. An unknown
    /// name reads as committed, which is where nothing will be found either
    /// way.
    pub fn glb(&self, root: &Path, name: &str) -> PathBuf {
        let publishable = self
            .animations
            .get(name)
            .is_none_or(|animation| animation.source.redistributable());
        let dir = if publishable {
            "art/animations"
        } else {
            "art/animations/local"
        };
        root.join(dir).join(format!("{name}.glb"))
    }

    /// Where a provider's own download is staged before it is fitted to the
    /// canonical rig. Derived and gitignored, and kept after a fetch so a bad
    /// one can be looked at.
    ///
    /// The extension is the caller's because it is the provider's: Mixamo
    /// exports FBX and Meshy delivers GLB.
    pub fn staged_download(root: &Path, name: &str, extension: &str) -> PathBuf {
        root.join("art/staging/downloads")
            .join(format!("{name}.{extension}"))
    }

    /// The armature every clip for this skeleton is authored against.
    ///
    /// A path rather than a declaration: a second skeleton is a second file
    /// and no code change. See `art/skeletons/README.md`.
    pub fn reference_rig(root: &Path, skeleton: &str) -> PathBuf {
        root.join("art/skeletons").join(format!("{skeleton}.glb"))
    }

    /// Loads the library, or an empty one if the project has no animations yet.
    pub fn load(root: &Path) -> Result<Self> {
        let path = Self::path(root);
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        ron::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        let path = Self::path(root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let config = ron::ser::PrettyConfig::new().struct_names(true);
        // ron omits the trailing newline; without it every write trips the
        // end-of-file pre-commit hook.
        let text = ron::ser::to_string_pretty(self, config)? + "\n";
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
    }

    /// Looks an animation up, naming what is available when it is missing.
    pub fn get(&self, name: &str) -> Result<&Animation> {
        self.animations.get(name).with_context(|| {
            let known: Vec<&str> = self.animations.keys().map(String::as_str).collect();
            format!(
                "no animation named {name:?} in the library. Available: {}",
                if known.is_empty() {
                    "none yet".to_owned()
                } else {
                    known.join(", ")
                }
            )
        })
    }

    /// Resolves animation names in order; the first defines standing height.
    /// An animation for another skeleton is rejected here, not at bake time.
    pub fn resolve<'a>(
        &'a self,
        names: &'a [String],
        skeleton: &str,
    ) -> Result<Vec<(&'a str, &'a Animation)>> {
        names
            .iter()
            .map(|name| {
                let animation = self.get(name)?;
                anyhow::ensure!(
                    animation.skeleton == skeleton,
                    "animation {name:?} is for the {:?} skeleton, but this character \
                     is rigged on {skeleton:?}, bone names would not match",
                    animation.skeleton
                );
                Ok((name.as_str(), animation))
            })
            .collect()
    }

    /// Every animation built for one skeleton.
    pub fn for_skeleton<'a>(&'a self, skeleton: &'a str) -> impl Iterator<Item = &'a str> {
        self.animations
            .iter()
            .filter(move |(_, animation)| animation.skeleton == skeleton)
            .map(|(name, _)| name.as_str())
    }

    /// The defaults `cargo art` writes when a project has no library yet.
    ///
    /// Every `source_fps` and every `travels` here is a measurement, listed
    /// beside the rule that reads it in the design document. The three Meshy
    /// clips carry keys 1/24 s apart, and only `walk_back` moves its hips
    /// anywhere: 1.271 m, against 0.000 for `run` and idle's 0.000.
    pub fn template() -> Self {
        Self {
            animations: BTreeMap::from([
                // Meshy animation-library ids. 544 is a walk, not a run.
                (
                    "idle".to_owned(),
                    Animation {
                        skeleton: HUMANOID.to_owned(),
                        loops: true,
                        fps: 8,
                        source_fps: 24,
                        travels: false,
                        source: MotionSource::Meshy { action_id: 251 },
                    },
                ),
                (
                    "run".to_owned(),
                    Animation {
                        skeleton: HUMANOID.to_owned(),
                        loops: true,
                        fps: 24,
                        source_fps: 24,
                        travels: false,
                        source: MotionSource::Meshy { action_id: 15 },
                    },
                ),
                (
                    "walk_back".to_owned(),
                    Animation {
                        skeleton: HUMANOID.to_owned(),
                        loops: true,
                        fps: 20,
                        source_fps: 24,
                        travels: true,
                        source: MotionSource::Meshy { action_id: 544 },
                    },
                ),
            ]),
        }
    }
}

/// What the retarget's gates said about one clip.
///
/// The vendor file is not committed, so this is the only record that an
/// uncommitted input passed them. Small on purpose: the findings themselves
/// are in the report this names, and a lock has to stay diffable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    /// `<stage>.<item>.<attempt>` under `art/staging/reports/`, which is
    /// where the attempt number is too.
    pub report: String,
    /// The worst thing any rule filed.
    pub worst: Severity,
    /// Every rule that reported, so a rule that went quiet is visible here
    /// too rather than only in a report nobody kept.
    pub rules: Vec<String>,
}

impl Verdict {
    /// Reads one off the report the retarget wrote, named by the same
    /// [`Artifacts`] the runner asked that script for.
    pub fn of(report: &Report, artifacts: &Artifacts) -> Self {
        let mut rules: Vec<String> = report
            .findings()
            .iter()
            .map(|finding| finding.rule.clone())
            .collect();
        rules.sort_unstable();
        rules.dedup();
        Self {
            report: artifacts.stem().to_owned(),
            worst: report.worst(),
            rules,
        }
    }

    /// Whether the gates refused this clip.
    pub fn failed(&self) -> bool {
        self.worst == Severity::Error
    }
}

/// The two files one fetch produced. Named, because both are bytes and
/// transposing them would record each under the other's fingerprint.
#[derive(Debug, Clone, Copy)]
pub struct ClipFiles<'a> {
    /// What the vendor sent.
    pub download: &'a [u8],
    /// What the retarget wrote from it.
    pub glb: &'a [u8],
}

/// What one fetch produced. Recorded so a re-run skips finished work, and so
/// a reviewer can tell which upstream motion made the file on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fetched {
    /// Where it came from, as the library declared it at the time.
    pub source: MotionSource,
    /// Fingerprint of the provider's own download, before any conversion.
    pub download: String,
    /// Fingerprint of the GLB the retarget wrote from it.
    pub glb: String,
    /// What fitted it: the canonical rig, its profile, the Blender build and
    /// the scripts, from [`crate::lock::blender_inputs`]. A record written
    /// before this existed carries none and is fetched again.
    #[serde(default)]
    pub fingerprint: String,
    /// What the gates said. A record with none is fetched again, because
    /// nothing says the file on disk was ever measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
}

/// Machine-owned record of every fetched clip: a sidecar to `library.ron`,
/// for the reason [`crate::lock`] gives, keep the hand-authored file diffable.
/// Keyed by library name, so it lines up with the library and with the GLB.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibraryLock {
    #[serde(default)]
    pub fetched: BTreeMap<String, Fetched>,
}

impl LibraryLock {
    pub fn path(root: &Path) -> PathBuf {
        root.join("art/animations/library.lock")
    }

    /// Loads the record, or an empty one if nothing has been fetched yet.
    pub fn load(root: &Path) -> Result<Self> {
        let path = Self::path(root);
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        ron::from_str(&text)
            .with_context(|| format!("parsing {} (delete it to re-fetch)", path.display()))
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        let path = Self::path(root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let config = ron::ser::PrettyConfig::new().struct_names(true);
        // ron omits the trailing newline; without it every write trips the
        // end-of-file pre-commit hook.
        let text = ron::ser::to_string_pretty(self, config)? + "\n";
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
    }

    /// Records one fetch, replacing whatever was there.
    pub fn record(
        &mut self,
        name: &str,
        source: MotionSource,
        files: ClipFiles<'_>,
        fingerprint: &str,
        verdict: Verdict,
    ) {
        self.fetched.insert(
            name.to_owned(),
            Fetched {
                source,
                download: crate::lock::digest(files.download),
                glb: crate::lock::digest(files.glb),
                fingerprint: fingerprint.to_owned(),
                verdict: Some(verdict),
            },
        );
    }
}
