//! `cargo art posture`: what a clip did to a body, in numbers.
//!
//! Measures only, no limits and no gates. Blender does the reading, because
//! a Mixamo source is an FBX and only Blender can open one.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail, ensure};

use crate::blender::{self, BLENDER_SRC};
use crate::check::aim::AimTable;
use crate::check::gltf_world::Skeleton;
use crate::check::profile::Profile;
use crate::check::{Artifacts, relative_to};
use crate::library::AnimationLibrary;

/// Nothing here retries, so every reading is the first attempt.
const FIRST_ATTEMPT: u32 = 1;

/// The name every posture artifact is filed under.
const STAGE: &str = "posture";

/// What `cargo art posture` was asked to read. Exactly one of the three, so
/// the argument says which file is opened and in whose naming.
#[derive(Debug, clap::Args)]
#[command(group(
    clap::ArgGroup::new("subject").required(true).args(["clip", "source", "rest"])
))]
pub struct Asked {
    /// A clip in the animation library, read as fitted on the canonical rig.
    pub clip: Option<String>,
    /// The same clip as the vendor delivered it, in the vendor's own naming.
    #[arg(long)]
    pub source: Option<String>,
    /// One rig's bind pose, by path.
    #[arg(long)]
    pub rest: Option<PathBuf>,
    /// Only these frames, comma separated. Every frame by default.
    #[arg(long, conflicts_with = "rest")]
    pub frames: Option<String>,
}

/// What one reading opens: the rig, the motion, and how that file names its
/// bones.
#[derive(Debug)]
struct Subject {
    /// What the artifacts are filed under, which is why a fitted clip and its
    /// source do not overwrite each other.
    item: String,
    skeleton: String,
    convention: String,
    /// The armature to read. Absent when the motion file carries its own.
    rig: Option<PathBuf>,
    /// The file holding the motion. Absent for a bind pose.
    clip: Option<PathBuf>,
    /// The rate the clip's own keys sit on, which the glTF importer needs
    /// before it turns key times in seconds into frames.
    source_fps: Option<u32>,
}

/// Prints the posture of one clip, or of one rig at rest, and says where the
/// same text was saved.
pub fn run(root: &Path, asked: &Asked) -> Result<()> {
    let script = root.join(BLENDER_SRC).join("read_posture.py");
    ensure!(
        script.exists(),
        "missing posture script at {}",
        script.display()
    );
    let subject = resolve(root, asked)?;
    let profile = Profile::of(root, &subject.skeleton)?;
    let axis = profile.child_axis.vector();
    let artifacts = Artifacts::new(root, STAGE, &subject.item, FIRST_ATTEMPT)?;
    let text = artifacts.readings();

    let mut args = vec![
        OsString::from("--skeleton"),
        Profile::path(root, &subject.skeleton).into(),
        OsString::from("--convention"),
        subject.convention.into(),
        OsString::from("--child-axis"),
        format!("{},{},{}", axis.x, axis.y, axis.z).into(),
        OsString::from("--out"),
        text.clone().into(),
    ];
    if let Some(rig) = &subject.rig {
        args.extend([OsString::from("--rig"), rig.into()]);
    }
    if let Some(clip) = &subject.clip {
        args.extend([OsString::from("--clip"), clip.into()]);
    }
    if let Some(fps) = subject.source_fps {
        args.extend([OsString::from("--source-fps"), fps.to_string().into()]);
    }
    if let Some(frames) = &asked.frames {
        args.extend([OsString::from("--frames"), frames.into()]);
    }

    blender::run(&script, &args, &artifacts, root)
        .with_context(|| format!("reading the posture of {}", subject.item))?;
    let printed = std::fs::read_to_string(&text).with_context(|| {
        format!(
            "the posture of {} wrote no {}",
            subject.item,
            text.display()
        )
    })?;
    print!("{printed}");
    println!("saved to {}", relative_to(&text, root));
    Ok(())
}

/// Which files one request reads, refusing a name or a path that is not
/// there rather than measuring nothing.
fn resolve(root: &Path, asked: &Asked) -> Result<Subject> {
    if let Some(rig) = &asked.rest {
        return at_rest(root, rig);
    }
    let library = AnimationLibrary::load(root)?;
    let name = asked
        .clip
        .as_deref()
        .or(asked.source.as_deref())
        .context("a clip name")?;
    let animation = library.get(name)?;
    let table = AimTable::of(root, &animation.skeleton)?;

    if asked.source.is_some() {
        // The same file a refit would read, so both agree on the vendor copy.
        let (from, convention) = crate::cli::clip_source(root, name, animation, table.canonical())
            .with_context(|| {
                format!("{name} has no vendor file on this machine, so there is nothing to read")
            })?;
        return Ok(Subject {
            item: format!("{name}_source"),
            skeleton: animation.skeleton.clone(),
            convention: convention.to_owned(),
            rig: None,
            clip: Some(from),
            source_fps: Some(animation.source_fps),
        });
    }

    let clip = library.glb(root, name);
    ensure!(
        clip.exists(),
        "{} is not on this machine, so {name} has no fit to read. \
         `cargo art fetch {name}` writes it",
        relative_to(&clip, root)
    );
    let rig = AnimationLibrary::reference_rig(root, &animation.skeleton);
    ensure!(
        rig.exists(),
        "{} is missing, and a fitted clip is read on the rig it was fitted to",
        relative_to(&rig, root)
    );
    Ok(Subject {
        item: name.to_owned(),
        skeleton: animation.skeleton.clone(),
        convention: table.canonical().to_owned(),
        rig: Some(rig),
        clip: Some(clip),
        source_fps: Some(animation.source_fps),
    })
}

/// One rig's bind pose, read in whichever convention its own bones name.
fn at_rest(root: &Path, rig: &Path) -> Result<Subject> {
    let skeleton = only_skeleton(root)?;
    let joints =
        Skeleton::read(rig).with_context(|| format!("reading the bones of {}", rig.display()))?;
    let names: Vec<&str> = joints
        .joints()
        .iter()
        .map(|joint| joint.name.as_str())
        .collect();
    let table = AimTable::of(root, &skeleton)?;
    let convention = table.convention_of(names.iter().copied())?;
    let stem = rig
        .file_stem()
        .and_then(|stem| stem.to_str())
        .with_context(|| format!("{} has no file name to file a reading under", rig.display()))?;
    Ok(Subject {
        item: format!("{stem}_rest"),
        convention: convention.to_owned(),
        skeleton,
        rig: Some(rig.to_path_buf()),
        clip: None,
        source_fps: None,
    })
}

/// The skeleton a rig handed over by path is read against.
///
/// Refused rather than guessed when this repository declares more than one:
/// two skeletons name their bones differently and nothing on the path says
/// which of them the file is.
fn only_skeleton(root: &Path) -> Result<String> {
    let declared = Profile::declared(root)?;
    match declared.as_slice() {
        [only] => Ok(only.clone()),
        [] => bail!("no skeleton in {}", Profile::dir(root).display()),
        many => bail!(
            "this repository declares {many:?}, and nothing in a bare path \
             says which of them a rig is. Read a clip by name instead"
        ),
    }
}
