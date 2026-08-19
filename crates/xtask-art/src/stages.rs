//! The pipeline stages themselves.
//!
//! Each stage is a function taking the spec and paths, producing files on disk
//! and a [`StageRecord`] for the lock. Stages never decide *whether* to run,
//! that is the driver's job in `main.rs`, so they stay easy to reason about
//! and to invoke individually.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail};

use crate::library::{AnimationLibrary, MotionSource};
use crate::lock::{StageRecord, TaskRef};
use crate::meshy::{self, Endpoint};
use crate::openai;
use crate::pack::{self, CharacterAssets};
use crate::preview;
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

/// Reconstructs a textured, retopologised mesh from the concept views. Remesh
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
        if AnimationLibrary::glb(root, name).exists() {
            println!("  {name}: already in the library");
            continue;
        }
        // Hand-authored motion is committed with the art; there is nothing to
        // buy and nothing to fetch.
        let MotionSource::Meshy { action_id } = animation.source else {
            println!("  {name}: authored, nothing to fetch");
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
pub async fn download(paths: &Paths, root: &Path, tasks: &[TaskRef]) -> Result<StageRecord> {
    let client = meshy::Client::from_env()?;
    let mut downloaded = 0;
    // A rigged character supersedes the bare mesh at the same path, so the
    // mesh is fetched only when nothing rigged it.
    let rigged = tasks.iter().any(|task| matches!(task, TaskRef::Rig { .. }));

    for task in tasks {
        let dest = match task {
            TaskRef::Model { .. } if rigged => continue,
            TaskRef::Model { .. } | TaskRef::Rig { .. } => paths.character_glb(),
            TaskRef::Animation { name, .. } => AnimationLibrary::glb(root, name),
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
        if matches!(task, TaskRef::Animation { .. }) {
            strip_animation(&dest, root)?;
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
fn strip_animation(glb: &Path, repo_root: &Path) -> Result<()> {
    let script = repo_root.join(BLENDER_SRC).join("strip_animation.py");
    anyhow::ensure!(
        script.exists(),
        "missing strip script at {}",
        script.display()
    );
    let mut command = blender_command_bare(&script, repo_root)?;
    command.arg("--glb").arg(glb);
    run_blender(command).with_context(|| format!("stripping {}", glb.display()))
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

    let mut command = blender_command(&script, paths, spec, repo_root)?;
    command.arg("--character").arg(&character);
    for (name, animation) in library.resolve(&spec.animations, &spec.subject.skeleton)? {
        let glb = AnimationLibrary::glb(repo_root, name);
        anyhow::ensure!(
            glb.exists(),
            "missing animation {}, run the download stage first",
            glb.display()
        );
        command
            .arg("--animation")
            .arg(format!("{name}={}", glb.display()))
            .arg("--fps")
            .arg(format!("{name}={}", animation.fps));
    }
    run_blender(command)?;

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

/// Where the bake script and the modules it imports live.
const BLENDER_SRC: &str = "tools/blender/src";

/// Site-packages of the project's virtualenv, handed to Blender's embedded
/// interpreter. The Python minor version must match Blender's, because
/// pydantic ships a compiled core, hence the glob.
pub fn venv_site_packages(repo_root: &Path) -> Result<PathBuf> {
    let lib = repo_root.join(".venv/lib");
    let mut candidates: Vec<(u32, u32, PathBuf)> = std::fs::read_dir(&lib)
        .with_context(|| {
            format!(
                "no virtualenv at {}, run `uv sync`",
                repo_root.join(".venv").display()
            )
        })?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let site = path.join("site-packages");
            if !site.is_dir() {
                return None;
            }
            // Sort on the parsed version, not the directory name: "python3.9"
            // sorts after "python3.13" as text, which would pick the older one.
            let name = path.file_name()?.to_str()?.strip_prefix("python")?;
            let (major, minor) = name.split_once('.')?;
            Some((major.parse().ok()?, minor.parse().ok()?, site))
        })
        .collect();
    candidates.sort_unstable();
    candidates
        .pop()
        .map(|(_, _, site)| site)
        .with_context(|| format!("no site-packages under {}, run `uv sync`", lib.display()))
}

/// Locates the Blender executable. Overridable for a test stub, or an
/// install outside PATH.
fn blender_binary() -> String {
    std::env::var("MARROWFALL_BLENDER_BIN").unwrap_or_else(|_| "blender".to_owned())
}

/// Blender, with the project's Python importable and nothing else assumed.
fn blender_command_bare(script: &Path, repo_root: &Path) -> Result<Command> {
    let python_path = std::env::join_paths([
        venv_site_packages(repo_root)?.into_os_string(),
        repo_root.join(BLENDER_SRC).into_os_string(),
    ])
    .context("building PYTHONPATH for blender")?;

    let mut command = Command::new(blender_binary());
    command
        .env("PYTHONPATH", python_path)
        .arg("--background")
        .arg("--python-use-system-env")
        .arg("--python")
        .arg(script)
        .arg("--");
    Ok(command)
}

/// The shared part of the Blender invocation.
fn blender_command(
    script: &Path,
    paths: &Paths,
    spec: &CharacterSpec,
    repo_root: &Path,
) -> Result<Command> {
    let mut command = blender_command_bare(script, repo_root)?;
    command
        .arg("--out")
        .arg(paths.staging())
        .arg("--directions")
        .arg(spec.bake.directions.to_string())
        .arg("--size")
        .arg(spec.bake.render_size.to_string())
        .arg("--trim-start")
        .arg(spec.bake.trim_start.to_string())
        .arg("--forearm-roll")
        .arg(spec.bake.forearm_roll.to_string());
    Ok(command)
}

fn run_blender(mut command: Command) -> Result<()> {
    let output = command
        .output()
        .context("running blender, is it on PATH?")?;
    if !output.status.success() {
        // Blender writes diagnostics to both streams; showing one of them
        // routinely hides the actual cause.
        bail!(
            "blender bake failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
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
