//! Shared test scaffolding.
//!
//! The pipeline reads its API keys and hosts from the environment, so pointing
//! it at a local server means setting process-global state. [`EnvGuard`]
//! serializes that and restores it, so tests in this binary cannot leak
//! settings into each other.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use xtask_art::check::profile::Profile;
use xtask_art::check::{Finding, Report, clip};
use xtask_art::library::AnimationLibrary;
use xtask_art::spec::{Bake, CharacterSpec, CharacterType, Remesh, Subject, Texture};

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// The repository itself, so a test reads the real committed art.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root")
}

/// One committed art file, checked to be the art rather than a pointer to
/// it. A checkout without Git LFS leaves text here, and every gate would
/// then measure a file that is not the asset.
pub fn committed_glb(file: &str) -> PathBuf {
    let path = repo_root().join(file);
    let bytes = std::fs::read(&path).unwrap_or_default();
    assert_eq!(
        bytes.get(..4),
        Some(b"glTF".as_slice()),
        "{file} is missing or is a Git LFS pointer, not a GLB. Run \
         `git lfs pull`, and in CI pass `lfs: true` to actions/checkout."
    );
    path
}

/// Holds the environment lock and undoes every variable it set on drop.
/// One per test: the lock is not re-entrant, so nesting two deadlocks.
pub struct EnvGuard {
    _lock: MutexGuard<'static, ()>,
    restore: Vec<(String, Option<String>)>,
}

impl EnvGuard {
    pub fn new() -> Self {
        Self {
            // A poisoned lock means another test panicked while holding it; the
            // variables were still restored by this type's Drop, so continuing
            // is correct.
            _lock: ENV_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            restore: Vec::new(),
        }
    }

    pub fn set(&mut self, key: &str, value: &str) -> &mut Self {
        self.restore.push((key.to_owned(), std::env::var(key).ok()));
        // SAFETY: every test touching the environment holds ENV_LOCK, so no
        // other thread in this binary is reading or writing it concurrently.
        unsafe { std::env::set_var(key, value) };
        self
    }

    pub fn remove(&mut self, key: &str) -> &mut Self {
        self.restore.push((key.to_owned(), std::env::var(key).ok()));
        // SAFETY: as above.
        unsafe { std::env::remove_var(key) };
        self
    }

    /// Points both providers at a local server and supplies dummy keys.
    pub fn with_api(&mut self, base: &str) -> &mut Self {
        self.set("MESHY_API_KEY", "test-key")
            .set("OPENAI_API_KEY", "test-key")
            .set("MARROWFALL_MESHY_BASE_URL", base)
            .set("MARROWFALL_OPENAI_BASE_URL", base)
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, previous) in self.restore.drain(..).rev() {
            // SAFETY: the lock is still held until this value is fully dropped.
            unsafe {
                match previous {
                    Some(value) => std::env::set_var(&key, value),
                    None => std::env::remove_var(&key),
                }
            }
        }
    }
}

/// A spec with the same defaults `cargo art new` writes, ready to vary.
pub fn a_spec(name: &str) -> CharacterSpec {
    CharacterSpec {
        name: name.to_owned(),
        subject: Subject {
            kind: CharacterType::Humanoid,
            description: "a lean, weathered survivor in torn shorts".to_owned(),
            height_meters: 1.7,
            skeleton: xtask_art::library::HUMANOID.to_owned(),
            cleanup: true,
            symmetry: true,
        },
        animations: vec!["idle".to_owned()],
        remesh: Remesh {
            target: 30_000,
            quads: true,
        },
        texture: Texture {
            pbr: true,
            resolution: xtask_art::spec::TextureResolution::K2,
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

/// A 2x2 PNG, small enough to inline and real enough to decode.
pub fn a_png() -> Vec<u8> {
    let mut bytes = Vec::new();
    let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encoding a 2x2 png");
    bytes
}

/// A concept view every `concept.*` rule holds: a flat backdrop, one figure,
/// and an arm clear of the ribcage on each side.
///
/// Small and synthetic, because a run test needs four views that pass the
/// gate and the committed art is 1.6 MB each. The same image serves all four
/// views, so `concept.cross_view` reads zero by construction.
pub fn a_concept_view() -> Vec<u8> {
    let mut image = image::RgbImage::from_pixel(128, 192, image::Rgb([138, 138, 138]));
    let skin = image::Rgb([170, 140, 110]);
    let mut block = |x: std::ops::Range<u32>, y: std::ops::Range<u32>| {
        for y in y {
            for x in x.clone() {
                image.put_pixel(x, y, skin);
            }
        }
    };
    block(56..72, 10..30); // head
    block(32..96, 30..38); // shoulders, which is what joins the arms on
    block(48..80, 38..110); // torso
    block(32..40, 38..110); // and one arm hanging clear on each side
    block(88..96, 38..110);
    block(52..60, 110..180); // legs
    block(68..76, 110..180);

    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encoding a concept view");
    bytes
}

/// The library every test resolves names against.
pub fn a_library() -> AnimationLibrary {
    AnimationLibrary::template()
}

/// Copies the committed skeleton profile into a temporary repo, because the
/// stages that publish a limit to Blender read it from there.
pub fn install_skeleton(root: &std::path::Path) {
    let skeletons = root.join("art/skeletons");
    std::fs::create_dir_all(&skeletons).expect("mkdir");
    let name = format!("{}.toml", xtask_art::library::HUMANOID);
    std::fs::copy(
        repo_root().join("art/skeletons").join(&name),
        skeletons.join(&name),
    )
    .expect("copying the committed skeleton profile");
}

/// The mesh as the model stage downloads it: textured, unskinned, and
/// carrying one hole and one piece of debris for the fixer to remove.
pub fn a_bare_mesh() -> Vec<u8> {
    crate::meshes::SyntheticMesh::figure()
        .without_a_face()
        .plus_debris()
        .to_glb()
}

/// And what a fixer writes from it: the same figure, whole.
pub fn a_cleaned_mesh() -> Vec<u8> {
    crate::meshes::SyntheticMesh::figure().to_glb()
}

/// Every subject the bake's own rules report on: one per axis of every clip.
pub fn bake_subjects(names: &[&str]) -> Vec<String> {
    names
        .iter()
        .flat_map(|name| clip::AXES.map(|axis| format!("{name} {axis}")))
        .collect()
}

/// One bake finding, built through the published rule so its limit, its
/// space and its severity are the real ones.
pub fn a_bake_finding(subject: &str, meters: f64) -> Finding {
    let profile = Profile::of(&repo_root(), xtask_art::library::HUMANOID)
        .expect("the committed humanoid profile");
    let rule = if subject.ends_with(" z") {
        &clip::ROOT_BOB
    } else {
        &clip::ROOT_TRAVEL
    };
    rule.measured(&profile, subject, meters, 1, "stub".to_owned())
}

/// One bake report, as JSON, out of findings a caller chose.
pub fn a_bake_report_of(item: &str, findings: Vec<Finding>) -> String {
    let mut report = Report::new("bake", item, 1);
    report.extend(findings).expect("published findings");
    serde_json::to_string(&report).expect("encoding the stub report")
}

/// And the report the bake writes when nothing is wrong.
pub fn a_bake_report(item: &str, names: &[&str]) -> String {
    let findings = bake_subjects(names)
        .iter()
        .map(|subject| a_bake_finding(subject, 0.0))
        .collect();
    a_bake_report_of(item, findings)
}

/// Writes the library plus a stub GLB for each animation, so a bake finds them.
pub fn install_library(root: &std::path::Path) -> AnimationLibrary {
    let library = a_library();
    library.save(root).expect("saving the library");
    for name in library.animations.keys() {
        let glb = library.glb(root, name);
        std::fs::create_dir_all(glb.parent().expect("glb has a parent")).expect("mkdir");
        std::fs::write(glb, b"glTF").expect("writing a stub animation");
    }
    library
}
