//! The pipeline stages themselves.
//!
//! Each stage is a function taking the spec and paths, producing files on disk
//! and a [`StageRecord`] for the lock. Stages never decide *whether* to run,
//! that is the driver's job in `main.rs`, so they stay easy to reason about
//! and to invoke individually.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use anyhow::{Context as _, Result};

use crate::blender::{self, BLENDER_SRC};
use crate::check::aim::AimTable;
use crate::check::profile::Profile;
use crate::check::{Artifacts, clip};
use crate::library::{Animation, AnimationLibrary, MotionSource};
use crate::lock::{Stage, StageRecord, TaskRef};
use crate::pack::{self, CharacterAssets};
use crate::preview;
use crate::providers::meshy::{self, Endpoint};
use crate::providers::openai;
use crate::spec::{CharacterSpec, Paths, View};

/// Generates the four concept views. Each is written as it arrives and reused
/// on a re-run, so a failure partway costs only the remaining views.
/// `force` regenerates all.
pub async fn concept(spec: &CharacterSpec, paths: &Paths, force: bool) -> Result<StageRecord> {
    let client = openai::Client::from_env()?;
    let pose = spec.subject.kind.pose_instruction();
    let concepts = paths.concept(View::Front);
    let concepts = concepts.parent().expect("concept path has a parent");
    std::fs::create_dir_all(concepts)
        .with_context(|| format!("creating {}", concepts.display()))?;

    // The front view seeds every other view, so it must exist first.
    let front_path = paths.concept(View::Front);
    let front = if front_path.exists() && !force {
        println!("  front: reusing existing");
        std::fs::read(&front_path).with_context(|| format!("reading {}", front_path.display()))?
    } else {
        println!("  generating front…");
        let bytes = client
            .generate(&openai::front_prompt(&spec.subject.description, pose))
            .await?;
        write_file(&front_path, &bytes)?;
        bytes
    };

    let mut generated = 0;
    for view in View::derived() {
        let path = paths.concept(view);
        if path.exists() && !force {
            println!("  {view}: reusing existing");
            continue;
        }
        println!("  generating {view}…");
        let bytes = client
            .edit(
                &openai::view_prompt(view, &spec.subject.description, pose),
                &front,
            )
            .await?;
        write_file(&path, &bytes)?;
        generated += 1;
    }

    preview::concept(paths)?;
    Ok(StageRecord {
        note: Some(format!(
            "{} views, {generated} newly generated",
            View::ALL.len()
        )),
        ..StageRecord::default()
    })
}

/// Reconstructs a textured, retopologized mesh from the concept views. Remesh
/// and texture are parameters here because the API performs them inline.
pub async fn model(spec: &CharacterSpec, paths: &Paths) -> Result<StageRecord> {
    let client = meshy::Client::from_env()?;

    let mut data_uris = Vec::new();
    for view in View::ALL {
        let path = paths.concept(view);
        let bytes = std::fs::read(&path).with_context(|| {
            format!(
                "missing concept {}, run the concept stage first",
                path.display()
            )
        })?;
        data_uris.push(meshy::to_data_uri(&bytes));
    }

    let task = client
        .run(
            Endpoint::MultiImageTo3d,
            meshy::image_to_3d_body(
                &data_uris,
                spec.remesh.target,
                spec.remesh.quads,
                spec.texture.pbr,
                spec.texture.resolution,
            ),
            |progress| println!("  mesh {progress}%"),
        )
        .await?;

    // Meshy renders turntable thumbnails for free; fetching them makes the
    // mesh reviewable without downloading it.
    let mut thumbnails = Vec::new();
    for url in task.thumbnail_urls() {
        if let Ok(bytes) = client.fetch(url).await {
            thumbnails.push(bytes);
        }
    }
    preview::model(paths, &thumbnails)?;

    Ok(StageRecord {
        tasks: vec![TaskRef::Model {
            id: task.id.clone(),
        }],
        credits: task.credits,
        ..StageRecord::default()
    })
}

/// Attaches a skeleton, then buys any animation the shared library lacks.
/// `on_task` persists each new one immediately, so a failure partway does not
/// discard what was already charged for.
pub async fn rig(
    spec: &CharacterSpec,
    library: &AnimationLibrary,
    root: &Path,
    already_done: &[TaskRef],
    mut on_task: impl FnMut(TaskRef),
) -> Result<StageRecord> {
    let client = meshy::Client::from_env()?;
    let height = spec.subject.height_meters;

    let reusable_rig = already_done.iter().find_map(|task| match task {
        TaskRef::Rig { id, height_meters } if *height_meters == height => Some(id.clone()),
        _ => None,
    });

    let rig_task = match reusable_rig {
        Some(existing) => {
            println!("  rig: reusing task {existing}");
            existing
        }
        None => {
            let model = already_done
                .iter()
                .find(|task| matches!(task, TaskRef::Model { .. }))
                .context("no model task recorded, run the model stage first")?;
            let task = client
                .run(
                    Endpoint::Rigging,
                    meshy::rigging_body(model.id(), height),
                    |progress| println!("  rig {progress}%"),
                )
                .await?;
            on_task(TaskRef::Rig {
                id: task.id.clone(),
                height_meters: height,
            });
            task.id
        }
    };

    let mut tasks = vec![TaskRef::Rig {
        id: rig_task.clone(),
        height_meters: height,
    }];
    for (name, animation) in library.resolve(&spec.animations, &spec.subject.skeleton)? {
        // The library is shared, so a motion another character already bought
        // costs nothing. This is the whole point of storing them centrally.
        if library.glb(root, name).exists() {
            println!("  {name}: already in the library");
            continue;
        }
        // Only Meshy sells motion attached to a rig it billed for. Anything
        // else is committed or fetched, and neither belongs in a paid stage.
        let MotionSource::Meshy { action_id } = animation.source else {
            println!("  {name}: not bought here, nothing to do");
            continue;
        };
        let reusable = already_done.iter().find(|task| {
            matches!(
                task,
                TaskRef::Animation { name: recorded, action_id: recorded_id, .. }
                    if recorded == name && *recorded_id == action_id
            )
        });
        if let Some(existing) = reusable {
            println!("  {name}: reusing task {}", existing.id());
            tasks.push(existing.clone());
            continue;
        }
        println!("  animating {name}…");
        let task = client
            .run(
                Endpoint::Animation,
                meshy::animation_body(&rig_task, action_id),
                |_| {},
            )
            .await
            .with_context(|| format!("animating {name:?}"))?;
        let reference = TaskRef::Animation {
            id: task.id,
            name: name.to_owned(),
            action_id,
        };
        on_task(reference.clone());
        tasks.push(reference);
    }

    Ok(StageRecord {
        tasks,
        ..StageRecord::default()
    })
}

/// Fetches the finished GLBs, the checkpoint everything downstream rebuilds
/// from: the rigged character, plus one file per animation.
pub async fn download(
    library: &AnimationLibrary,
    paths: &Paths,
    root: &Path,
    tasks: &[TaskRef],
) -> Result<StageRecord> {
    let client = meshy::Client::from_env()?;
    let mut downloaded = 0;
    // A rigged character supersedes the bare mesh at the same path, so the
    // mesh is fetched only when nothing rigged it.
    let rigged = tasks.iter().any(|task| matches!(task, TaskRef::Rig { .. }));

    for task in tasks {
        let dest = match task {
            TaskRef::Model { .. } if rigged => continue,
            TaskRef::Model { .. } | TaskRef::Rig { .. } => paths.character_glb(),
            TaskRef::Animation { name, .. } => library.glb(root, name),
        };
        let status = client.status(task.endpoint(), task.id()).await?;
        let url = status.glb_url().with_context(|| {
            format!(
                "task {} is {:?} and exposes no GLB url",
                task.id(),
                status.status
            )
        })?;
        client.download(url, &dest).await?;
        // Providers ship the whole character with each animation; committing
        // ~5 MB of mesh and texture per motion would be permanent in git.
        if let TaskRef::Animation { name, .. } = task {
            strip_animation(&dest, name, root)?;
        }
        downloaded += 1;
    }

    anyhow::ensure!(
        downloaded > 0,
        "nothing to download, run the model and rig stages first"
    );
    Ok(StageRecord {
        note: Some(format!("{downloaded} GLB(s)")),
        ..StageRecord::default()
    })
}

/// Rewrites an animation GLB with only its armature and action.
fn strip_animation(glb: &Path, name: &str, repo_root: &Path) -> Result<()> {
    let script = repo_root.join(BLENDER_SRC).join("strip_animation.py");
    anyhow::ensure!(
        script.exists(),
        "missing strip script at {}",
        script.display()
    );
    let args = vec![OsString::from("--glb"), glb.into()];
    let artifacts = Artifacts::new(repo_root, Stage::Download.as_str(), name, FIRST_ATTEMPT)?;
    let findings = blender::run(&script, &args, &artifacts, repo_root)
        .with_context(|| format!("stripping {}", glb.display()))?;
    anyhow::ensure!(
        findings.is_none(),
        "the strip reported findings nobody reads yet"
    );
    Ok(())
}

/// Fits a downloaded clip onto the skeleton's canonical rig, writing the
/// animation GLB the bake reads.
pub fn retarget(
    source: &Path,
    out: &Path,
    name: &str,
    animation: &Animation,
    repo_root: &Path,
) -> Result<()> {
    let script = repo_root.join(BLENDER_SRC).join("retarget_animation.py");
    anyhow::ensure!(
        script.exists(),
        "missing retarget script at {}",
        script.display()
    );
    let skeleton = &animation.skeleton;
    let rig = AnimationLibrary::reference_rig(repo_root, skeleton);
    anyhow::ensure!(
        rig.exists(),
        "no canonical rig for the {skeleton:?} skeleton at {}. \
         art/skeletons/README.md says where that file comes from",
        rig.display()
    );

    // Not a `Stage`: the retarget runs inside the fetch path and the lock
    // keeps its six stages. It still gets its own report.
    let artifacts = Artifacts::new(repo_root, "retarget", name, FIRST_ATTEMPT)?;
    let args = vec![
        OsString::from("--source"),
        source.into(),
        OsString::from("--rig"),
        rig.clone().into(),
        OsString::from("--convention"),
        animation.source.bone_convention().into(),
        OsString::from("--out"),
        out.into(),
        OsString::from("--name"),
        name.into(),
        OsString::from("--source-motion"),
        artifacts.source_motion().into(),
    ];
    let mut report = blender::run(&script, &args, &artifacts, repo_root)
        .with_context(|| format!("retargeting {}", source.display()))?
        .with_context(|| format!("the retarget of {name} wrote no report"))?;
    let profile = Profile::of(repo_root, skeleton)?;
    let off = report.off_registry(&profile);
    anyhow::ensure!(
        off.is_empty(),
        "the retarget of {name} reported findings the rule list does not \
         carry, so nothing published what they were read against: {}",
        off.join("; ")
    );
    // Blender counted what only an action carries. These three read the file
    // it exported, which is where a wrong export would otherwise hide.
    report.extend(clip::check_files(
        out,
        &artifacts.source_motion(),
        &rig,
        repo_root,
        &profile,
        &AimTable::of(repo_root, skeleton)?,
        FIRST_ATTEMPT,
    )?)?;
    report.write(repo_root)?;
    anyhow::ensure!(
        !report.has_errors(),
        "the retarget of {name} left {} defect(s), listed in {}",
        report
            .findings()
            .iter()
            .filter(|finding| !finding.holds())
            .count(),
        artifacts.report().display()
    );
    Ok(())
}

/// Renders sprite frames via headless Blender. One invocation for every
/// animation, because the camera is sized from the widest pose: framing per
/// invocation would change the character's size between animations.
pub fn bake(
    spec: &CharacterSpec,
    library: &AnimationLibrary,
    paths: &Paths,
    repo_root: &Path,
) -> Result<StageRecord> {
    let script = repo_root.join(BLENDER_SRC).join("bake_sprites.py");
    anyhow::ensure!(
        script.exists(),
        "missing bake script at {}",
        script.display()
    );
    anyhow::ensure!(
        !spec.animations.is_empty(),
        "{} has no animations to bake. Meshy's rigger only supports bipedal \
         humanoids, so a {:?} character currently has no sprites to produce.",
        spec.name,
        spec.subject.kind
    );

    let character = paths.character_glb();
    anyhow::ensure!(
        character.exists(),
        "no character at {}, run the download stage first",
        character.display()
    );

    if paths.staging().exists() {
        // Stale frames from a previous shape would be picked up by packing.
        std::fs::remove_dir_all(paths.staging())
            .with_context(|| format!("clearing {}", paths.staging().display()))?;
    }

    let mut args = bake_args(paths, spec);
    args.push("--character".into());
    args.push(character.into());
    for (name, animation) in library.resolve(&spec.animations, &spec.subject.skeleton)? {
        let glb = library.glb(repo_root, name);
        anyhow::ensure!(
            glb.exists(),
            "missing animation {}, {}",
            glb.display(),
            match animation.source {
                MotionSource::Meshy { .. } => "run the download stage first".to_owned(),
                MotionSource::Mixamo { .. } => format!("get it with `cargo art fetch {name}`"),
                MotionSource::Authored => "author it and commit it".to_owned(),
            }
        );
        args.push("--animation".into());
        args.push(blender::pair(name, glb));
        args.push("--fps".into());
        args.push(blender::pair(name, animation.fps.to_string()));
    }
    let artifacts = Artifacts::new(repo_root, Stage::Bake.as_str(), &spec.name, FIRST_ATTEMPT)?;
    anyhow::ensure!(
        blender::run(&script, &args, &artifacts, repo_root)?.is_none(),
        "the bake reported findings nobody reads yet"
    );

    let names: Vec<&str> = library
        .resolve(&spec.animations, &spec.subject.skeleton)?
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    preview::bake(&names, pack::direction_names(spec.bake.directions)?, paths)?;

    let frames = std::fs::read_dir(paths.staging())
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "png"))
                .count()
        })
        .unwrap_or(0);
    Ok(StageRecord {
        note: Some(format!("{frames} frames")),
        ..StageRecord::default()
    })
}

/// Nothing but the concept stage retries, so every other report is the first
/// and only attempt.
const FIRST_ATTEMPT: u32 = 1;

/// The bake parameters the spec fixes, ahead of the per-animation arguments.
fn bake_args(paths: &Paths, spec: &CharacterSpec) -> Vec<OsString> {
    vec![
        "--out".into(),
        paths.staging().into(),
        "--directions".into(),
        spec.bake.directions.to_string().into(),
        "--size".into(),
        spec.bake.render_size.to_string().into(),
        "--trim-start".into(),
        spec.bake.trim_start.to_string().into(),
        "--forearm-roll".into(),
        spec.bake.forearm_roll.to_string().into(),
    ]
}

/// Crops, scales and packs the baked frames, then writes the manifest. One
/// stage, because the manifest describes the layout packing just produced.
pub fn pack(
    spec: &CharacterSpec,
    library: &AnimationLibrary,
    paths: &Paths,
) -> Result<StageRecord> {
    let directions = pack::direction_names(spec.bake.directions)?;
    std::fs::create_dir_all(paths.assets())
        .with_context(|| format!("creating {}", paths.assets().display()))?;

    let resolved = library.resolve(&spec.animations, &spec.subject.skeleton)?;
    let loaded: Vec<(String, Vec<pack::Frame>)> = resolved
        .iter()
        .map(|(name, _)| {
            pack::load_animation_frames(&paths.staging(), name, directions)
                .map(|frames| ((*name).to_owned(), frames))
        })
        .collect::<Result<_>>()?;

    // One crop and scale across every animation, so the character cannot change
    // size between animations.
    let character = pack::character_scale(
        loaded.iter().map(|(_, frames)| frames.as_slice()),
        spec.bake.sprite_height,
    )?;

    let mut animations = BTreeMap::new();
    for (name, frames) in &loaded {
        let file = format!("{name}.png");
        let animation = library.get(name)?;
        let (atlas, layout) = pack::pack_animation(
            frames,
            directions,
            file.clone(),
            animation.fps,
            animation.loops,
            &character,
        )?;
        let dest = paths.assets().join(&file);
        atlas
            .save(&dest)
            .with_context(|| format!("writing atlas {}", dest.display()))?;
        pack::write_import_settings(&dest)?;
        animations.insert(name.clone(), layout);
    }

    let assets = CharacterAssets {
        name: spec.name.clone(),
        animations,
    };
    let config = ron::ser::PrettyConfig::new().struct_names(true);
    let manifest = paths.assets().join("character.ron");
    // ron omits the trailing newline; without it every write trips the
    // end-of-file pre-commit hook.
    let text = ron::ser::to_string_pretty(&assets, config)? + "\n";
    std::fs::write(&manifest, text).with_context(|| format!("writing {}", manifest.display()))?;

    preview::sprites(&assets, paths)?;
    Ok(StageRecord {
        note: Some(format!(
            "{} atlases → {}",
            assets.animations.len(),
            paths.relative(&manifest)
        )),
        ..StageRecord::default()
    })
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}
