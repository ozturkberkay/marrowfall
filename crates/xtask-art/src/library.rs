//! The shared animation library: motion declared once, reused by every
//! character on the same skeleton, the way Godot and Unity share clips.
//!
//! Sharing works because almost all of a clip is bone *rotation*, which is
//! independent of proportions. The exception is `location`, which varies on
//! `Hips`, `LeftShoulder` and `neck` and is in the units of the rig it was
//! bought against, so a run's vertical bob is sized for that character.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

/// This project's standard biped: 24 bones, Mixamo naming, no fingers.
/// A hand-rigged humanoid must match these bone names to be the same skeleton.
pub const HUMANOID: &str = "humanoid";

/// Where a motion comes from, and so how its file gets on disk. Nothing
/// downstream asks; adding a provider is a new variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MotionSource {
    /// Retargeted from Meshy's animation library, selected by numeric id.
    /// Bought once, then shared by every character on the same skeleton.
    Meshy { action_id: u32 },
    /// Hand-authored, in Blender, or anywhere else, and committed with the
    /// rest of the art. There is nothing to fetch and nothing to pay for.
    Authored,
}

impl MotionSource {
    /// Whether obtaining this motion costs money.
    pub const fn costs_credits(&self) -> bool {
        matches!(self, Self::Meshy { .. })
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
    pub fn glb(root: &Path, name: &str) -> PathBuf {
        root.join("art/animations").join(format!("{name}.glb"))
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
                        source: MotionSource::Meshy { action_id: 251 },
                    },
                ),
                (
                    "run".to_owned(),
                    Animation {
                        skeleton: HUMANOID.to_owned(),
                        loops: true,
                        fps: 24,
                        source: MotionSource::Meshy { action_id: 15 },
                    },
                ),
                (
                    "walk_back".to_owned(),
                    Animation {
                        skeleton: HUMANOID.to_owned(),
                        loops: true,
                        fps: 20,
                        source: MotionSource::Meshy { action_id: 544 },
                    },
                ),
            ]),
        }
    }
}
