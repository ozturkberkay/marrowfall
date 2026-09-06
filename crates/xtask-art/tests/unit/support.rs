//! Shared test scaffolding.
//!
//! The pipeline reads its API keys and hosts from the environment, so pointing
//! it at a local server means setting process-global state. [`EnvGuard`]
//! serializes that and restores it, so tests in this binary cannot leak
//! settings into each other.

use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard};

use xtask_art::blender::Build;
use xtask_art::check::profile::Profile;
use xtask_art::check::{Finding, Report, bake, clip};
use xtask_art::library::AnimationLibrary;
use xtask_art::lock::Inputs;
use xtask_art::spec::{Bake, CharacterSpec, CharacterType, Paths, Remesh, Subject, Texture, View};

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
            pose_mode: None,
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
            directions: A_SPEC_DIRECTIONS,
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

/// Copies the committed skeleton into a temporary repo: the profile, because
/// the stages that publish a limit to Blender read it from there, and the
/// canonical rig beside it, because every fingerprint of a Blender stage
/// reads both.
pub fn install_skeleton(root: &std::path::Path) {
    let skeletons = root.join("art/skeletons");
    std::fs::create_dir_all(&skeletons).expect("mkdir");
    for extension in ["toml", "glb"] {
        let name = format!("{}.{extension}", xtask_art::library::HUMANOID);
        std::fs::copy(
            repo_root().join("art/skeletons").join(&name),
            skeletons.join(&name),
        )
        .expect("copying the committed skeleton");
    }
}

/// And the Blender scripts, which are the other half of what a fingerprint
/// reads before it can say a bake is current.
pub fn install_scripts(root: &std::path::Path) {
    let to = root.join("tools/blender/src");
    std::fs::create_dir_all(&to).expect("mkdir");
    for entry in std::fs::read_dir(repo_root().join("tools/blender/src")).expect("the scripts") {
        let from = entry.expect("a script").path();
        if from.extension().is_some_and(|ext| ext == "py") {
            std::fs::copy(&from, to.join(from.file_name().expect("a file name")))
                .expect("copying a Blender script");
        }
    }
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

/// Every subject the bake's own two travel rules report on: one per axis of
/// every clip.
pub fn bake_subjects(names: &[&str]) -> Vec<String> {
    names
        .iter()
        .flat_map(|name| clip::AXES.map(|axis| format!("{name} {axis}")))
        .collect()
}

/// The ring `a_spec` bakes, and the two directions a golden is taken in.
pub fn a_ring() -> &'static [&'static str] {
    xtask_art::pack::direction_names(A_SPEC_DIRECTIONS).expect("a known ring")
}

pub fn golden_directions() -> [&'static str; 2] {
    bake::golden_directions(a_ring()).expect("two golden directions")
}

/// How many directions `a_spec` renders. Eight, so a golden pair is `s` and
/// `e` and the reflection every `bake.pivot` reading needs exists.
pub const A_SPEC_DIRECTIONS: u32 = 8;

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

/// Every finding `bake_sprites.py` writes when nothing is wrong: the two
/// travel rules per axis, the sampled frames of each clip, and one golden per
/// clip per golden direction.
pub fn a_bake_findings(names: &[&str]) -> Vec<Finding> {
    let profile = Profile::of(&repo_root(), xtask_art::library::HUMANOID)
        .expect("the committed humanoid profile");
    let mut findings: Vec<Finding> = bake_subjects(names)
        .iter()
        .map(|subject| a_bake_finding(subject, 0.0))
        .collect();
    for name in names {
        findings.push(bake::SAMPLED_FRAMES_ARE_KEYS.measured(
            &profile,
            name,
            0.0,
            1,
            "stub".to_owned(),
        ));
        for direction in golden_directions() {
            findings.push(bake::LANDMARK_GOLDEN.measured(
                &profile,
                &bake::golden(name, direction),
                0.0,
                1,
                "stub".to_owned(),
            ));
        }
    }
    findings
}

/// And that report, as the JSON a stub hands back.
pub fn a_bake_report(item: &str, names: &[&str]) -> String {
    a_bake_report_of(item, a_bake_findings(names))
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

// --- What a fingerprint reads off disk ------------------------------------

/// The Blender build a test declares rather than looks up: nothing in this
/// suite may need Blender installed.
pub const BLENDER: &str = "Blender 4.5.3 LTS";

/// And that build as a fingerprint reads it, which asks no binary anything.
pub static A_BUILD: LazyLock<Build> = LazyLock::new(|| Build::stated(BLENDER));

/// The first line of every `blender` stub: a command that fingerprints a
/// Blender stage asks for the build before it runs anything.
pub fn answers_its_version() -> String {
    format!("case \"$1\" in --version) echo \"{BLENDER}\"; exit 0;; esac\n")
}

/// A `blender` that answers `--version` and refuses everything else, for a
/// command that reads the build without running a script.
pub fn a_version_only_blender(dir: &Path) -> PathBuf {
    let stub = dir.join("blender-version.sh");
    std::fs::write(
        &stub,
        format!("#!/bin/sh\n{}exit 1\n", answers_its_version()),
    )
    .expect("writing the stub");
    std::fs::set_permissions(
        &stub,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .expect("making the stub executable");
    stub
}

/// Everything a fingerprint reads, over a tree a test built.
pub fn inputs<'a>(
    root: &'a Path,
    spec: &'a CharacterSpec,
    library: &'a AnimationLibrary,
) -> Inputs<'a> {
    Inputs {
        root,
        spec,
        library,
        blender: &A_BUILD,
    }
}

/// A repo-shaped tree holding the real committed inputs a fingerprint reads,
/// so a test can edit exactly one of them and read the pair.
pub fn a_tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    install_skeleton(root);
    install_scripts(root);

    let library = a_library();
    library.save(root).unwrap();
    for name in library.animations.keys() {
        let glb = library.glb(root, name);
        std::fs::create_dir_all(glb.parent().unwrap()).unwrap();
        std::fs::copy(
            repo_root()
                .join("art/animations")
                .join(format!("{name}.glb")),
            glb,
        )
        .unwrap();
    }
    let paths = Paths::new(root, "survivor");
    std::fs::create_dir_all(paths.dir()).unwrap();
    for view in View::ALL {
        let png = paths.concept(view);
        std::fs::create_dir_all(png.parent().unwrap()).unwrap();
        std::fs::write(png, a_png()).unwrap();
    }
    // A stand-in, not the committed 4.8 MB mesh: only its bytes are read.
    std::fs::write(paths.character_glb(), b"glTF the character").unwrap();
    dir
}

/// Points the head's aim at another axis: one `[aim_table]` row, which is
/// what the transfer reads to fit a clip.
///
/// The row is matched whole, because `head` also names a bone in the two
/// convention tables above it.
pub fn edit_an_aim_row(root: &Path) {
    let path = root.join("art/skeletons/humanoid.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    let row = "head = [0.0, 0.0, 1.0]";
    assert_eq!(
        text.matches(row).count(),
        1,
        "exactly one [aim_table] row to edit"
    );
    std::fs::write(&path, text.replace(row, "head = [0.0, 1.0, 0.0]")).unwrap();
}

/// Flips the last byte of a file: the smallest edit a content hash has to
/// see, and one that leaves the name, the size and the date alone.
pub fn flip_a_byte(path: &std::path::Path) {
    let mut bytes = std::fs::read(path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(path, bytes).unwrap();
}
