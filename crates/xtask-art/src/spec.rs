//! The character spec: the hand-authored source of truth for one character.
//! Everything the pipeline does derives from it.
//!
//! - **creative**, [`Subject::description`]
//! - **structural**, [`Subject::kind`], the skeleton, the animation list
//! - **locked defaults**, remesh, texture and bake settings
//!
//! Machine state lives in a separate lock file, so this stays diffable.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

/// A view of the character, for the concept references a reconstructor needs.
/// Ours, not a provider's: each service is told about them in its own words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum View {
    /// Generated first and used as the reference for the rest, so every view
    /// shows recognisably the same character.
    Front,
    Back,
    Left,
    Right,
}

impl View {
    pub const ALL: [View; 4] = [View::Front, View::Back, View::Left, View::Right];

    /// Views derived from the front reference.
    pub fn derived() -> impl Iterator<Item = View> {
        Self::ALL.into_iter().filter(|view| *view != View::Front)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            View::Front => "front",
            View::Back => "back",
            View::Left => "left",
            View::Right => "right",
        }
    }
}

impl std::fmt::Display for View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Texture map size. A plain measurement, not any provider's spelling of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextureResolution {
    K2,
    K4,
    K8,
}

/// Body plan. Drives the concept pose and whether the character can be
/// auto-rigged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
pub enum CharacterType {
    /// Two arms, two legs. The only body plan Meshy can auto-rig.
    Humanoid,
    /// Four legs. Generated and textured normally, but Meshy's rigger targets
    /// bipeds, so these ship as static sprites until hand-rigged.
    Quadruped,
    /// Anything else, floating, amorphous, many-limbed.
    Other,
}

impl CharacterType {
    /// The pose instruction injected into the concept prompt.
    ///
    /// A-pose for humanoids: it matches the bind pose Meshy's animation rig
    /// uses, and angling the arms down puts the top of the forearms in view of
    /// the front camera, which a T-pose hides from every reference.
    pub fn pose_instruction(self) -> &'static str {
        match self {
            Self::Humanoid => {
                "A-pose: arms straight, angled 40 degrees DOWN and OUT from the torso, \
                 fully separated from the body with a clear gap of empty background \
                 between each arm and the ribcage. Forearms in NEUTRAL rotation: palms \
                 face INWARD toward the thighs, thumbs pointing FORWARD, not \
                 palms-forward and not palms-down. Fingers straight, slightly apart, not \
                 touching the legs. Legs straight, feet shoulder-width apart with a clear \
                 gap between them. Head level, facing dead front. Perfectly symmetrical."
            }
            Self::Quadruped => {
                "Standing square on all four legs, body level, legs straight and clearly \
                 separated from each other with visible gaps between them. Head level and \
                 facing forward. Tail (if any) hanging straight down, not overlapping the body."
            }
            Self::Other => {
                "Neutral resting pose, symmetrical, with every limb, tendril or appendage \
                 clearly separated from the body mass and from each other."
            }
        }
    }

    /// Whether an auto-rigger can handle this body plan. Only bipeds today,
    /// so everything else skips rigging and animation.
    pub const fn can_be_rigged(self) -> bool {
        matches!(self, Self::Humanoid)
    }
}

/// What the character looks like. Only [`Self::description`] is free-form
/// creative text; everything else in the pipeline is mechanical.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subject {
    pub kind: CharacterType,
    /// Free-form prose describing the character. Injected verbatim into the
    /// concept prompt, so write it the way you would describe the character
    /// to an artist.
    pub description: String,
    /// Real-world height, used to scale the rig.
    pub height_meters: f32,
    /// The skeleton this character is rigged onto. It can only play motion
    /// built for the same one, since animations address bones by name.
    pub skeleton: String,
}

/// Retopology settings applied during generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Remesh {
    /// Target face count. Meshy's rigger rejects meshes over 300k, and raw
    /// generation output routinely exceeds a million.
    pub target: u32,
    /// `true` for quads, which deform cleanly at joints; triangles pinch.
    pub quads: bool,
}

/// Texture generation settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Texture {
    /// Generate the full PBR set. The normal map is what lets flat sprites
    /// respond to the game's dynamic lighting.
    pub pbr: bool,
    pub resolution: TextureResolution,
}

/// Sprite bake settings, passed through to the Blender script.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bake {
    /// Compass directions rendered, evenly spaced. 8 is the Diablo II layout.
    pub directions: u32,
    /// Render size in pixels before downscaling.
    pub render_size: u32,
    /// Final in-game sprite height in pixels.
    pub sprite_height: u32,
    /// Sprite frames sampled per second. Counts follow from this and each
    /// animation's duration, so every one plays at its authored speed from a
    /// Degrees of forearm roll correction, for models whose bind pose has
    /// supinated (palms-forward) arms.
    pub forearm_roll: f32,
    /// Fraction of each animation skipped at the start, for generated motions that
    /// ramp in from a neutral pose.
    pub trim_start: f32,
}

/// A complete character definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CharacterSpec {
    /// Identifier used for directories, filenames and the game manifest.
    pub name: String,
    pub subject: Subject,
    /// Animations to bake, by name, resolved against the shared library. Only
    /// the choice is per character. Order matters: the first entry defines
    /// standing height.
    pub animations: Vec<String>,
    pub remesh: Remesh,
    pub texture: Texture,
    pub bake: Bake,
}

impl CharacterSpec {
    /// Defaults matching the settings validated while building the survivor.
    pub fn template(name: &str, kind: CharacterType) -> Self {
        Self {
            name: name.to_owned(),
            subject: Subject {
                kind,
                description: "TODO: describe the character".to_owned(),
                height_meters: 1.7,
                skeleton: crate::library::HUMANOID.to_owned(),
            },
            animations: if kind.can_be_rigged() {
                vec!["idle".to_owned(), "run".to_owned()]
            } else {
                Vec::new()
            },
            remesh: Remesh {
                target: 30_000,
                quads: true,
            },
            texture: Texture {
                pbr: true,
                resolution: TextureResolution::K2,
            },
            bake: Bake {
                directions: 8,
                render_size: 256,
                sprite_height: 160,
                forearm_roll: 0.0,
                trim_start: 0.0,
            },
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading spec {}", path.display()))?;
        ron::from_str(&text).with_context(|| format!("parsing spec {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let config = ron::ser::PrettyConfig::new().struct_names(true);
        // ron omits the trailing newline; without it every write trips the
        // end-of-file pre-commit hook.
        let text = ron::ser::to_string_pretty(self, config)? + "\n";
        std::fs::write(path, text).with_context(|| format!("writing spec {}", path.display()))
    }

    /// Rejects specs that would fail late, in the middle of a paid pipeline.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(!self.name.is_empty(), "name must not be empty");
        anyhow::ensure!(
            self.name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
            "name must be lowercase ascii, digits or underscore: {:?}",
            self.name
        );
        anyhow::ensure!(
            !self.subject.description.starts_with("TODO"),
            "spec still has the placeholder description, describe the character first"
        );
        anyhow::ensure!(
            self.subject.height_meters > 0.0,
            "height_meters must be positive, got {}",
            self.subject.height_meters
        );
        // Kept in step with `pack::direction_names`, which is the only place
        // that can name the rows of an atlas. Failing here beats failing after
        // the paid stages have already run.
        anyhow::ensure!(
            matches!(self.bake.directions, 4 | 8),
            "directions must be 4 or 8, got {}",
            self.bake.directions
        );
        anyhow::ensure!(
            (0.0..1.0).contains(&self.bake.trim_start),
            "trim_start must be in 0.0..1.0, got {}",
            self.bake.trim_start
        );
        anyhow::ensure!(
            (100..=300_000).contains(&self.remesh.target),
            "remesh target {} outside Meshy's supported 100..=300000 range",
            self.remesh.target
        );
        anyhow::ensure!(
            self.bake.sprite_height <= self.bake.render_size,
            "sprite_height {} must not exceed render_size {}",
            self.bake.sprite_height,
            self.bake.render_size
        );

        if self.subject.kind.can_be_rigged() {
            anyhow::ensure!(
                !self.animations.is_empty(),
                "at least one animation required"
            );
        } else {
            anyhow::ensure!(
                self.animations.is_empty(),
                "Meshy's auto-rigger only supports bipedal humanoids, so a {:?} \
                 character cannot have animations, remove them and it will ship \
                 as a static sprite",
                self.subject.kind
            );
        }

        // Names are checked here; that they exist is checked against the
        // library, which is where the useful error can name the alternatives.
        let mut seen = std::collections::HashSet::new();
        for name in &self.animations {
            anyhow::ensure!(!name.is_empty(), "animation name must not be empty");
            anyhow::ensure!(seen.insert(name), "duplicate animation {name:?}");
        }
        Ok(())
    }
}

/// Directory layout for one character. Centralised so every stage agrees on
/// where things live.
pub struct Paths {
    pub root: PathBuf,
    pub name: String,
}

impl Paths {
    pub fn new(root: impl Into<PathBuf>, name: &str) -> Self {
        Self {
            root: root.into(),
            name: name.to_owned(),
        }
    }

    /// Everything committed about one character, in one directory. Derived
    /// output stays outside it, so committed art never mixes with throwaway.
    pub fn dir(&self) -> PathBuf {
        self.root.join("art/characters").join(&self.name)
    }

    /// Hand-authored spec.
    pub fn spec(&self) -> PathBuf {
        self.dir().join("spec.ron")
    }

    /// Machine-owned progress record.
    pub fn lock(&self) -> PathBuf {
        self.dir().join("spec.lock")
    }

    /// Expensive, non-reproducible AI output. Committed to git.
    pub fn concept(&self, view: View) -> PathBuf {
        self.dir().join("concept").join(format!("{view}.png"))
    }

    /// The character: mesh, skeleton and textures, stored once.
    pub fn character_glb(&self) -> PathBuf {
        self.dir().join("model.glb")
    }

    /// Raw bake output. Derived; gitignored.
    pub fn staging(&self) -> PathBuf {
        self.root.join(format!("art/staging/{}", self.name))
    }

    /// Reviewable artefacts, one per stage. Derived; gitignored.
    pub fn preview(&self) -> PathBuf {
        self.root.join(format!("art/preview/{}", self.name))
    }

    /// Game-ready assets consumed by Godot.
    pub fn assets(&self) -> PathBuf {
        self.root
            .join(format!("project/assets/characters/{}", self.name))
    }

    /// Renders an absolute path relative to the repo root, so notes stored in
    /// the committed lock file are not machine-specific.
    pub fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .display()
            .to_string()
    }
}
