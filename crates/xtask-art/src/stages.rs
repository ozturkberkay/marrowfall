//! The pipeline stages themselves.
//!
//! Each stage is a function taking the spec and paths, producing files on disk
//! and a [`StageRecord`] for the lock. Stages never decide *whether* to run,
//! that is the driver's job in `main.rs`, so they stay easy to reason about
//! and to invoke individually.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use crate::blender::{self, BLENDER_SRC};
use crate::check::aim::{self, AimTable};
use crate::check::gltf_world::Skeleton;
use crate::check::profile::Profile;
use crate::check::{
    Artifacts, Finding, Report, Rule, Severity, Symmetry, atlas, bake as bake_check, clip, concept,
    gltf_mesh, mesh, rig, source,
};
use crate::godot;
use crate::library::{Animation, AnimationLibrary, MotionSource, Verdict};
use crate::lock::{Stage, StageRecord, TaskRef};
use crate::pack::{self, CharacterAssets};
use crate::preview;
use crate::providers::meshy::{self, Endpoint};
use crate::providers::openai;
use crate::spec::{CharacterSpec, Paths, View};

/// Generates all four concept views, every time it runs.
///
/// Nothing on disk is reused. The retry loop needs a fresh set or a retry
/// would re-measure the images that just failed, and a new front view
/// invalidates the three derived from it anyway. Whether this runs at all is
/// the plan's decision in `cli.rs`, and the lock is the only cache.
pub async fn concept(spec: &CharacterSpec, paths: &Paths) -> Result<StageRecord> {
    let client = openai::Client::from_env()?;
    let pose = spec.subject.kind.pose_instruction();
    let concepts = paths.concept(View::Front);
    let concepts = concepts.parent().expect("concept path has a parent");
    std::fs::create_dir_all(concepts)
        .with_context(|| format!("creating {}", concepts.display()))?;

    // The front view seeds every other view, so it comes first.
    println!("  generating front…");
    let front = client
        .generate(&openai::front_prompt(&spec.subject.description, pose))
        .await?;
    write_file(&paths.concept(View::Front), &front)?;

    for view in View::derived() {
        println!("  generating {view}…");
        let bytes = client
            .edit(
                &openai::view_prompt(view, &spec.subject.description, pose),
                &front,
            )
            .await?;
        write_file(&paths.concept(view), &bytes)?;
    }

    preview::concept(paths)?;
    Ok(StageRecord {
        note: Some(format!("{} views generated", View::ALL.len())),
        ..StageRecord::default()
    })
}

/// Measures the four concept views and writes
/// `art/staging/reports/concept.<char>.<attempt>.json`.
///
/// The retry loop runs this after every generation and `cargo art check` runs
/// it on whatever is on disk: one producer, one rule set, one stage name. The
/// report is on disk before any of it is refused, so a run that stops on a
/// gate still leaves what it measured for a human to read.
pub fn check_concept(
    spec: &CharacterSpec,
    paths: &Paths,
    repo_root: &Path,
    attempt: u32,
) -> Result<Report> {
    let profile = Profile::of(repo_root, &spec.subject.skeleton)?;
    let files: Vec<(View, PathBuf)> = View::ALL
        .into_iter()
        .map(|view| (view, paths.concept(view)))
        .collect();
    let views: Vec<concept::Rendered<'_>> = files
        .iter()
        .map(|(view, file)| concept::Rendered {
            name: view.as_str(),
            file,
            torso_faces_the_camera: view.shows_the_torso(),
        })
        .collect();

    let mut report = Report::new(concept::STAGE, &spec.name, attempt);
    report.extend(concept::check_files(
        &views,
        &profile,
        Symmetry::declared(spec.subject.symmetry),
        attempt,
    ))?;
    report.write(repo_root)?;
    refuse_unread_rules(&report, &concept::RULES, &spec.name, "concept check")?;
    refuse_unreported_subjects(
        &report,
        &concept::subjects(&views),
        &spec.name,
        "concept check",
    )?;
    Ok(report)
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

    let submitted = Artifacts::new(
        &paths.root,
        Stage::Model.as_str(),
        paths.item(),
        FIRST_ATTEMPT,
    )?;
    let task = client
        .run(
            Endpoint::MultiImageTo3d,
            meshy::image_to_3d_body(
                &data_uris,
                spec.remesh.target,
                spec.remesh.quads,
                spec.texture.pbr,
                spec.texture.resolution,
                spec.subject.pose_mode,
            ),
            &submitted.task(),
            |progress| println!("  mesh {progress}%"),
        )
        .await?;

    // The mesh gates and the fixer both run on the bare mesh, so the stage
    // that generated it is the stage that fetches it. Cleaning the rigged
    // file instead would desync the skin weights it carries (fact 10).
    let url = task
        .glb_url()
        .context("the finished model task exposes no GLB url")?;
    client.download(url, &paths.bare_glb()).await?;

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
        note: Some(format!(
            "bare mesh at {}",
            paths.relative(&paths.bare_glb())
        )),
        ..StageRecord::default()
    })
}

/// Measures the bare mesh, cleans it, and measures what the fixer wrote.
///
/// Returns the file sent to rigging: `clean.glb`, or the bare mesh itself when
/// the spec declares no cleanup. Both files are held to every file rule,
/// because the one that gets rigged is the one that has to have passed. The
/// fixer measures nothing, which is why this refuses a report it wrote
/// nothing into.
pub fn clean_mesh(spec: &CharacterSpec, paths: &Paths, repo_root: &Path) -> Result<PathBuf> {
    let bare = paths.bare_glb();
    anyhow::ensure!(
        bare.exists(),
        "no bare mesh at {}, run the model stage first",
        paths.relative(&bare)
    );
    let profile = Profile::of(repo_root, &spec.subject.skeleton)?;
    let artifacts = Artifacts::new(repo_root, mesh::CLEANUP_STAGE, paths.item(), FIRST_ATTEMPT)?;
    let cleaned = spec
        .subject
        .cleanup
        .then(|| run_fixer(spec, paths, repo_root, &profile, &artifacts))
        .transpose()?;

    // `print/analyze` needs a task or a URL and this runs on files, so the
    // remote rule has nothing to read either time.
    let read = |file: &Path| {
        mesh::check_file(
            file,
            repo_root,
            &profile,
            f64::from(spec.subject.height_meters),
            Symmetry::declared(spec.subject.symmetry),
            None,
            FIRST_ATTEMPT,
        )
    };
    // The mesh as it arrived, then the mesh the fixer wrote, then the pair.
    // The second is measured because the file that gets rigged is the file
    // that has to have passed.
    let mut measured = vec![(mesh::STAGE, "mesh", &mesh::FILE_RULES[..], read(&bare)?)];
    if let Some(clean) = &cleaned {
        measured.push((
            mesh::CLEANED_STAGE,
            "cleaned mesh",
            &mesh::CLEANED_RULES[..],
            read(clean)?,
        ));
    }
    measured.push((
        mesh::CLEANUP_STAGE,
        "cleanup",
        &mesh::CLEANUP_RULES[..],
        mesh::check_cleanup_files(
            &bare,
            cleaned.as_deref(),
            repo_root,
            &profile,
            FIRST_ATTEMPT,
        ),
    ));

    // Every report on disk before any of them is refused: a stage that stops
    // on a gate still leaves what it measured for a human to read.
    let mut filed = Vec::new();
    for (stage, what, rules, findings) in measured {
        let mut report = Report::new(stage, paths.item(), FIRST_ATTEMPT);
        report.extend(mesh::only(rules, findings))?;
        filed.push((report.write(repo_root)?, report, rules, what));
    }
    for (written, report, rules, what) in filed {
        // The defects first: an unreadable file leaves most rules with
        // nothing to read, and "the file is broken" is the useful half.
        anyhow::ensure!(
            !report.has_errors(),
            "the {what} of {} left {}, listed in {}. Rigging is not worth its \
             credits on a mesh that fails a gate",
            spec.name,
            defects(&report),
            written.display()
        );
        refuse_unread_rules(&report, rules, &spec.name, what)?;
    }
    Ok(cleaned.unwrap_or(bare))
}

/// Runs the Blender fixer and returns what it wrote.
fn run_fixer(
    spec: &CharacterSpec,
    paths: &Paths,
    repo_root: &Path,
    profile: &Profile,
    artifacts: &Artifacts,
) -> Result<PathBuf> {
    let script = repo_root.join(BLENDER_SRC).join("mesh_clean.py");
    anyhow::ensure!(
        script.exists(),
        "missing the mesh fixer at {}",
        script.display()
    );
    let clean = paths.clean_glb();
    let args = vec![
        OsString::from("--glb"),
        paths.bare_glb().into(),
        OsString::from("--out"),
        clean.clone().into(),
        OsString::from("--meshes"),
        profile.meshes.join(",").into(),
        // The distance the gates weld at, so the fixer merges exactly what
        // they count. The other two are profile data.
        OsString::from("--weld"),
        gltf_mesh::WELD_METERS.to_string().into(),
        OsString::from("--island-volume"),
        profile
            .cleanup
            .smallest_island_cubic_meters
            .to_string()
            .into(),
        OsString::from("--symmetry-threshold"),
        profile.cleanup.symmetrize_meters.to_string().into(),
        OsString::from("--symmetry"),
        spec.subject.symmetry.to_string().into(),
    ];
    let findings = blender::run(&script, &args, artifacts, repo_root)
        .with_context(|| format!("cleaning {}", paths.relative(&paths.bare_glb())))?;
    anyhow::ensure!(
        findings.is_none(),
        "the fixer wrote a report, and the fixer measures nothing: \
         check/mesh.rs is what reads the pair it produced"
    );
    anyhow::ensure!(
        clean.exists(),
        "the fixer finished and wrote no {}",
        paths.relative(&clean)
    );
    Ok(clean)
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
            // The cleaned mesh goes to the rigger by value. Rigging by task
            // id would rig the mesh Meshy generated instead, and everything
            // the fixer did would be thrown away.
            let mesh = clean_mesh(spec, &Paths::new(root, &spec.name), root)?;
            let glb =
                std::fs::read(&mesh).with_context(|| format!("reading {}", mesh.display()))?;
            let submitted = Artifacts::new(root, Stage::Rig.as_str(), &spec.name, FIRST_ATTEMPT)?;
            let task = client
                .run(
                    Endpoint::Rigging,
                    meshy::rigging_body(&meshy::to_model_uri(&glb), height),
                    &submitted.task(),
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
        let submitted = Artifacts::new(root, Stage::Rig.as_str(), name, FIRST_ATTEMPT)?;
        let task = client
            .run(
                Endpoint::Animation,
                meshy::animation_body(&rig_task, action_id),
                &submitted.task(),
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
///
/// A rigged character lands in staging as `rigged.glb` and reaches
/// `model.glb` through [`conform_rig`], because the vendor names its bones
/// its own way and points its joints wherever it likes. Each animation lands
/// beside it as a vendor download and reaches `art/animations/` through
/// [`check_source`] and [`retarget`], for the same reason.
pub async fn download(
    spec: &CharacterSpec,
    library: &AnimationLibrary,
    paths: &Paths,
    root: &Path,
    tasks: &[TaskRef],
) -> Result<StageRecord> {
    // A rigged character supersedes the bare mesh at the same path, so the
    // mesh is fetched only when nothing rigged it.
    let rigged = tasks.iter().any(|task| matches!(task, TaskRef::Rig { .. }));
    let wanted: Vec<(&TaskRef, PathBuf)> = tasks
        .iter()
        .filter_map(|task| {
            let dest = match task {
                TaskRef::Model { .. } if rigged => return None,
                TaskRef::Model { .. } => paths.character_glb(),
                TaskRef::Rig { .. } => paths.rigged_glb(),
                // Meshy delivers a clip as GLB, and it is a vendor download
                // rather than the library's file until the fit.
                TaskRef::Animation { name, .. } => {
                    AnimationLibrary::staged_download(root, name, "glb")
                }
            };
            Some((task, dest))
        })
        .collect();
    anyhow::ensure!(
        !wanted.is_empty(),
        "nothing to download, run the model and rig stages first"
    );

    // The vendor's own bytes are kept, so re-running this stage after a local
    // fix asks the provider for nothing: a task URL expires and the rename,
    // the conform and the fit are all offline work on files already here.
    let missing: Vec<(&TaskRef, &Path)> = wanted
        .iter()
        .filter(|(_, dest)| !dest.exists())
        .map(|(task, dest)| (*task, dest.as_path()))
        .collect();
    if !missing.is_empty() {
        let client = meshy::Client::from_env()?;
        for (task, dest) in missing {
            let status = client.status(task.endpoint(), task.id()).await?;
            let url = status.glb_url().with_context(|| {
                format!(
                    "task {} is {:?} and exposes no GLB url",
                    task.id(),
                    status.status
                )
            })?;
            client.download(url, dest).await?;
        }
    }

    if rigged {
        conform_rig(spec, paths, root)?;
    }
    for (task, download) in &wanted {
        if let TaskRef::Animation { name, .. } = task {
            fit_clip(library, root, name, download)?;
        }
    }
    Ok(StageRecord {
        note: Some(format!("{} GLB(s)", wanted.len())),
        ..StageRecord::default()
    })
}

/// Takes one bought clip from the rig the vendor animated onto ours, which is
/// the path a Mixamo clip takes.
///
/// The delivered file carries the vendor's names and the vendor's rest pose,
/// and an action's keys are read against a rest pose that [`conform_rig`] has
/// just moved. The fit also drops the stock character the clip arrives with.
fn fit_clip(library: &AnimationLibrary, root: &Path, name: &str, download: &Path) -> Result<()> {
    let animation = library.get(name)?;
    let convention = animation.source.bone_convention();
    // Before the retarget: the vendor's own rest geometry is on record from
    // here, and nothing downstream still carries it.
    check_source(download, name, animation, convention, root)?;
    // The verdict has no reader here: the download stage records no clip of
    // its own, and `cargo art fetch` is what refits one later. Its report is
    // on disk either way.
    retarget(
        download,
        &library.glb(root, name),
        name,
        animation,
        convention,
        root,
    )?;
    Ok(())
}

/// Takes the rigged file the vendor returned to the conformant character
/// `model.glb`, measuring it at both ends.
///
/// The rules run twice under two stage names, the way `mesh` and `cleaned`
/// read the mesh before and after the fixer. The `rig` report is a record of
/// what arrived and refuses nothing: a bought rig fails the three name rules
/// by construction, and the rename is what closes them. The `conformed`
/// report is the gate.
pub fn conform_rig(spec: &CharacterSpec, paths: &Paths, repo_root: &Path) -> Result<()> {
    let rigged = paths.rigged_glb();
    let bytes =
        std::fs::read(&rigged).with_context(|| format!("reading {}", paths.relative(&rigged)))?;
    check_rig(spec, paths, repo_root, rig::STAGE, &rigged, &bytes)?;

    let table = AimTable::of(repo_root, &spec.subject.skeleton)?;
    let named = crate::conform::rename(&bytes, &table)
        .with_context(|| format!("renaming the bones of {}", paths.relative(&rigged)))?;
    // Before the geometry is touched: every rule below reads the profile by
    // bone name, so a rename that half worked would measure the wrong joints.
    refuse_defects(
        &named,
        spec,
        repo_root,
        &rig::NAME_RULES,
        "the rename of",
        &paths.relative(&rigged),
    )?;

    let profile = Profile::of(repo_root, &spec.subject.skeleton)?;
    let conformed =
        crate::conform::conform(&named, &profile, Symmetry::declared(spec.subject.symmetry))
            .with_context(|| format!("conforming {}", paths.relative(&rigged)))?;

    let out = paths.character_glb();
    write_file(&out, &conformed)?;
    let report = check_rig(
        spec,
        paths,
        repo_root,
        rig::CONFORMED_STAGE,
        &out,
        &conformed,
    )?;
    anyhow::ensure!(
        !report.has_errors(),
        "the conformed rig of {} left {}, listed in {}",
        spec.name,
        defects(&report),
        report.artifacts(repo_root)?.report().display()
    );
    println!("  conformed rig at {}", paths.relative(&out));
    Ok(())
}

/// Promotes a conformed character to its skeleton's canonical rig, the file
/// every clip for that skeleton is fitted onto.
///
/// Deliberate and rare: it replaces shared art, so every clip in the library
/// is refitted afterwards, which `cargo art fetch` is what does. The rig
/// gates run first, because a canonical rig that fails one is a defect every
/// character on that skeleton inherits.
pub fn promote_rig(spec: &CharacterSpec, paths: &Paths, repo_root: &Path) -> Result<PathBuf> {
    let character = paths.character_glb();
    let bytes = std::fs::read(&character)
        .with_context(|| format!("reading {}", paths.relative(&character)))?;
    let report = check_rig(
        spec,
        paths,
        repo_root,
        rig::CONFORMED_STAGE,
        &character,
        &bytes,
    )?;
    anyhow::ensure!(
        !report.has_errors(),
        "{} left {}, and a canonical rig is shared by every character on the \
         {:?} skeleton, listed in {}",
        paths.relative(&character),
        defects(&report),
        spec.subject.skeleton,
        report.artifacts(repo_root)?.report().display()
    );

    let script = repo_root.join(BLENDER_SRC).join("promote_rig.py");
    anyhow::ensure!(
        script.exists(),
        "missing the promotion script at {}",
        script.display()
    );
    let out = AnimationLibrary::reference_rig(repo_root, &spec.subject.skeleton);
    let artifacts = Artifacts::new(repo_root, PROMOTE_STAGE, paths.item(), FIRST_ATTEMPT)?;
    let args = vec![
        OsString::from("--character"),
        character.clone().into(),
        OsString::from("--out"),
        out.clone().into(),
    ];
    let findings = blender::run(&script, &args, &artifacts, repo_root)
        .with_context(|| format!("promoting {}", paths.relative(&character)))?;
    anyhow::ensure!(
        findings.is_none(),
        "the promotion wrote a report, and it measures nothing: the rig rules \
         above are what read the rig it copies"
    );

    let promoted = Skeleton::read(&out)?;
    let moved = furthest_joint(&Skeleton::from_slice(&bytes)?, &promoted)?;
    anyhow::ensure!(
        moved <= PROMOTION_METERS,
        "the promoted rig's joints sit {moved:.2e} m from the character's, \
         over the {PROMOTION_METERS:.0e} m a GLB round trip can explain"
    );
    println!(
        "  canonical rig at {}, joints moved {moved:.2e} m",
        paths.relative(&out)
    );
    Ok(out)
}

/// How far the furthest shared joint of two rigs sits apart, and an error
/// when one of them is missing a joint the other has.
fn furthest_joint(character: &Skeleton, promoted: &Skeleton) -> Result<f64> {
    let mut worst = 0.0_f64;
    for joint in promoted.joints() {
        let same = character.get(&joint.name).with_context(|| {
            format!(
                "the promoted rig carries {:?} and the character does not",
                joint.name
            )
        })?;
        worst = worst.max(joint.position().distance(same.position()));
    }
    anyhow::ensure!(
        promoted.joints().len() == character.joints().len(),
        "the character has {} joints and the promoted rig {}",
        character.joints().len(),
        promoted.joints().len()
    );
    Ok(worst)
}

/// Every `rig.*` rule on one rig in memory, filed under one stage name.
///
/// The convention comes from the file's own `[fingerprints]` rather than from
/// the canonical table, because a bought rig is named the vendor's way until
/// the rename and reading it as ours resolves `spine_lower` to the wrong bone.
pub fn check_rig(
    spec: &CharacterSpec,
    paths: &Paths,
    repo_root: &Path,
    stage: &str,
    file: &Path,
    bytes: &[u8],
) -> Result<Report> {
    let profile = Profile::of(repo_root, &spec.subject.skeleton)?;
    let table = AimTable::of(repo_root, &spec.subject.skeleton)?;
    let subject = paths.relative(file);

    // A file no rule can read still leaves a report, and it is one error
    // rather than silence: a gate that goes quiet on unreadable input proves
    // nothing. Nothing below it ran, so nothing below it is owed either.
    let refused = |rule: &Rule, why: String| -> Result<Report> {
        let mut report = Report::new(stage, paths.item(), FIRST_ATTEMPT);
        report.add(rule.undefined(&subject, FIRST_ATTEMPT, why))?;
        report.write(repo_root)?;
        Ok(report)
    };
    let skeleton = match Skeleton::from_slice(bytes) {
        Ok(skeleton) => skeleton,
        Err(error) => {
            return refused(
                &rig::BONE_SET,
                format!("{subject} holds no readable skeleton: {error:#}"),
            );
        }
    };
    let joints: Vec<&str> = skeleton
        .joints()
        .iter()
        .map(|joint| joint.name.as_str())
        .collect();
    let convention = match table.convention_of(joints.iter().copied()) {
        Ok(convention) => convention,
        Err(error) => return refused(&rig::NAMES_STANDARD, format!("{error:#}")),
    };

    let mut report = Report::new(stage, paths.item(), FIRST_ATTEMPT);
    report.extend(rig::check(
        &subject,
        &skeleton,
        &profile,
        f64::from(spec.subject.height_meters),
        Symmetry::declared(spec.subject.symmetry),
        FIRST_ATTEMPT,
    ))?;
    report.extend(aim::check(
        &skeleton,
        &profile,
        &table,
        convention,
        FIRST_ATTEMPT,
    )?)?;
    report.write(repo_root)?;
    refuse_unread_rules(&report, &rig::RULES, paths.item(), "rig check")?;
    refuse_unread_rules(&report, &aim::RULES, paths.item(), "rig check")?;
    // Every joint the file carries, so a reader that quietly drops one is a
    // failing stage rather than a shorter report.
    let owed: Vec<String> = joints.iter().map(|name| (*name).to_owned()).collect();
    refuse_unreported_subjects(&report, &owed, paths.item(), "rig check")?;
    Ok(report)
}

/// Refuses a rig on a named subset of the rig rules, without filing a report:
/// the step files one at each end and this is the check in between.
fn refuse_defects(
    bytes: &[u8],
    spec: &CharacterSpec,
    repo_root: &Path,
    rules: &[&Rule],
    what: &str,
    file: &str,
) -> Result<()> {
    let profile = Profile::of(repo_root, &spec.subject.skeleton)?;
    let skeleton = Skeleton::from_slice(bytes)?;
    let broken: Vec<String> = rig::check(
        file,
        &skeleton,
        &profile,
        f64::from(spec.subject.height_meters),
        Symmetry::declared(spec.subject.symmetry),
        FIRST_ATTEMPT,
    )
    .into_iter()
    .filter(|finding| {
        finding.severity == Severity::Error && rules.iter().any(|rule| rule.id == finding.rule)
    })
    .map(|finding| format!("{} on {}", finding.rule, finding.subject))
    .collect();
    anyhow::ensure!(
        broken.is_empty(),
        "{what} {file} left {}, so no profile row can say which joint it is \
         about",
        broken.join(", ")
    );
    Ok(())
}

/// Measures a vendor clip before anything is fitted to it.
///
/// Between the download and the retarget, on the file as it arrived. Nothing
/// downstream still holds the vendor's own geometry, because the transfer
/// keeps our bone names and our rest pose, so this is the only boundary where
/// `source.child_axis` can be read at all.
pub fn check_source(
    download: &Path,
    name: &str,
    animation: &Animation,
    convention: &str,
    repo_root: &Path,
) -> Result<()> {
    let script = repo_root.join(BLENDER_SRC).join("check_source.py");
    anyhow::ensure!(
        script.exists(),
        "missing source check script at {}",
        script.display()
    );
    let skeleton = &animation.skeleton;
    let profile = Profile::of(repo_root, skeleton)?;
    let children = mapped_children(&profile, &AimTable::of(repo_root, skeleton)?)?;
    let axis = profile.child_axis.vector();
    let mut args = vec![
        OsString::from("--source"),
        download.into(),
        OsString::from("--skeleton"),
        Profile::path(repo_root, skeleton).into(),
        OsString::from("--convention"),
        convention.into(),
        OsString::from("--source-fps"),
        animation.source_fps.to_string().into(),
        OsString::from("--travels"),
        animation.travels.to_string().into(),
        OsString::from("--children"),
        children.into(),
        OsString::from("--child-axis"),
        format!("{},{},{}", axis.x, axis.y, axis.z).into(),
    ];
    args.extend(published(source::RULES, &profile));

    let artifacts = Artifacts::new(repo_root, source::STAGE, name, FIRST_ATTEMPT)?;
    let report = blender::run(&script, &args, &artifacts, repo_root)
        .with_context(|| format!("measuring {}", download.display()))?
        .with_context(|| format!("the source check of {name} wrote no report"))?;
    refuse_off_registry(&report, &profile, name, "source check")?;
    refuse_unread_rules(&report, &source::RULES, name, "source check")?;
    anyhow::ensure!(
        !report.has_errors(),
        "{name} is not the clip the library declares it is, {}, listed in {}",
        defects(&report),
        artifacts.report().display()
    );
    Ok(())
}

/// `hips=spine_lower,...`: which role each bone's own axis points at, read
/// off `[profile.tails]` and turned into roles, which is the only key a
/// vendor rig also carries.
///
/// Both Blender scripts take the same table, and that is the point: the
/// source gates report how far a vendor bone sits from the direction to its
/// own child, and the retarget is what takes that difference out.
fn mapped_children(profile: &Profile, table: &AimTable) -> Result<String> {
    Ok(
        source::mapped_children(profile, table.bones(table.canonical())?)
            .into_iter()
            .map(|(role, child)| format!("{role}={child}"))
            .collect::<Vec<String>>()
            .join(","),
    )
}

/// `--limit RULE=NUMBER` for every rule a script reports, read off the rule
/// list itself.
///
/// A published limit is profile data that Rust validates once, so no Blender
/// script opens a skeleton file to find one, and none of them can report a
/// limit the list does not carry.
fn published(rules: impl IntoIterator<Item = &'static Rule>, profile: &Profile) -> Vec<OsString> {
    rules
        .into_iter()
        .flat_map(|rule| {
            [
                OsString::from("--limit"),
                blender::pair(rule.id, (rule.limit)(profile).to_string()),
            ]
        })
        .collect()
}

/// Refuses a report whose findings disagree with what `--list-rules` prints.
fn refuse_off_registry(report: &Report, profile: &Profile, name: &str, what: &str) -> Result<()> {
    let off = report.off_registry(profile);
    anyhow::ensure!(
        off.is_empty(),
        "the {what} of {name} reported findings the rule list does not \
         carry, so nothing published what they were read against: {}",
        off.join("; ")
    );
    Ok(())
}

/// Refuses a report that carries no finding for one of the rules the stage
/// owes: the ones a Blender script was published a limit for, or the whole
/// family a Rust pass measures.
///
/// A gate that goes quiet cannot be told from one that never ran, so deleting
/// the call that measures is a failing stage rather than a quiet pass.
fn refuse_unread_rules(report: &Report, rules: &[&Rule], item: &str, what: &str) -> Result<()> {
    let seen: BTreeSet<&str> = report
        .findings()
        .iter()
        .map(|finding| finding.rule.as_str())
        .collect();
    let missing: Vec<&str> = rules
        .iter()
        .map(|rule| rule.id)
        .filter(|id| !seen.contains(id))
        .collect();
    anyhow::ensure!(
        missing.is_empty(),
        "the {what} of {item} reported no finding under {}, so that rule was \
         never read: a gate that goes quiet cannot be told from one that \
         never ran",
        missing.join(", ")
    );
    Ok(())
}

/// Refuses a report that named none of its findings after one of the subjects
/// the stage owed: an axis of a clip the bake was given, a concept view.
///
/// The companion of [`refuse_unread_rules`]: that one catches a rule nobody
/// read, this one a subject nobody read it on.
fn refuse_unreported_subjects(
    report: &Report,
    owed: &[String],
    item: &str,
    what: &str,
) -> Result<()> {
    let seen: BTreeSet<&str> = report
        .findings()
        .iter()
        .map(|finding| finding.subject.as_str())
        .collect();
    let missing: Vec<&str> = owed
        .iter()
        .map(String::as_str)
        .filter(|subject| !seen.contains(subject))
        .collect();
    anyhow::ensure!(
        missing.is_empty(),
        "the {what} of {item} named no finding after the subject(s) {}, so \
         nothing was read on them: a gate that goes quiet cannot be told \
         from one that never ran",
        missing.join(", ")
    );
    Ok(())
}

/// A report's defects, counted and named: "2 defect(s) on rule.a, rule.b".
///
/// Errors only, which is what [`Report::has_errors`] gates on. A `skipped`
/// rule chose not to measure and files a reading of zero, so counting it
/// would name a number no reader can find in the report. The rules go in
/// too, because a count alone says nothing about what to open.
pub(crate) fn defects(report: &Report) -> String {
    let broken: Vec<&Finding> = report
        .findings()
        .iter()
        .filter(|finding| finding.severity == Severity::Error)
        .collect();
    let rules: BTreeSet<&str> = broken.iter().map(|finding| finding.rule.as_str()).collect();
    format!(
        "{} defect(s) on {}",
        broken.len(),
        rules.into_iter().collect::<Vec<&str>>().join(", ")
    )
}

/// Fits a downloaded clip onto the skeleton's canonical rig, writing the
/// animation GLB the bake reads, and returns what its gates said.
///
/// The verdict is what `library.lock` stores: the vendor file is not
/// committed, so nothing else records that an uncommitted input was measured.
pub fn retarget(
    source: &Path,
    out: &Path,
    name: &str,
    animation: &Animation,
    convention: &str,
    repo_root: &Path,
) -> Result<Verdict> {
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
    // Written to staging and copied into place only once the gates pass, so a
    // refused fit never lands where the bake reads it. The staged copy stays
    // behind either way, which is what the refusal below points a human at.
    let fitted = AnimationLibrary::staged_fit(repo_root, name);
    let mut args = vec![
        OsString::from("--source"),
        source.into(),
        OsString::from("--rig"),
        rig.clone().into(),
        OsString::from("--convention"),
        convention.into(),
        OsString::from("--out"),
        fitted.clone().into(),
        OsString::from("--name"),
        name.into(),
        OsString::from("--source-motion"),
        artifacts.source_motion().into(),
        OsString::from("--source-fps"),
        animation.source_fps.to_string().into(),
        OsString::from("--travels"),
        animation.travels.to_string().into(),
    ];
    let profile = Profile::of(repo_root, skeleton)?;
    let table = AimTable::of(repo_root, skeleton)?;
    args.extend([
        OsString::from("--children"),
        mapped_children(&profile, &table)?.into(),
    ]);
    args.extend(published(clip::RETARGET_RULES, &profile));
    let mut report = blender::run(&script, &args, &artifacts, repo_root)
        .with_context(|| format!("retargeting {}", source.display()))?
        .with_context(|| format!("the retarget of {name} wrote no report"))?;
    refuse_off_registry(&report, &profile, name, "retarget")?;
    refuse_unread_rules(&report, &clip::RETARGET_RULES, name, "retarget")?;
    // Blender counted what only the action and the pose it evaluated carry.
    // These read the file it exported, which is where a wrong export would
    // otherwise hide.
    report.extend(clip::check_files(
        &clip::Fitted {
            output: &fitted,
            source_motion: &artifacts.source_motion(),
            rig: &rig,
            repo_root,
            source_fps: animation.source_fps,
            loops: animation.loops,
            travels: animation.travels,
        },
        &profile,
        &table,
        FIRST_ATTEMPT,
    )?)?;
    report.write(repo_root)?;
    anyhow::ensure!(
        !report.has_errors(),
        "the retarget of {name} left {}, listed in {}. The fit itself is at {}",
        defects(&report),
        artifacts.report().display(),
        fitted.display()
    );
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(&fitted, out)
        .with_context(|| format!("copying {} to {}", fitted.display(), out.display()))?;
    Ok(Verdict::of(&report, &artifacts))
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

    clear_frames(&paths.staging())?;

    let mut args = bake_args(paths, spec);
    args.push("--character".into());
    args.push(character.into());
    let clips = library.resolve(&spec.animations, &spec.subject.skeleton)?;
    let names: Vec<&str> = clips.iter().map(|(name, _)| *name).collect();
    for (name, animation) in clips {
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
    let directions = pack::direction_names(spec.bake.directions)?;
    let goldens = bake_check::golden_directions(directions).with_context(|| {
        format!(
            "a ring of {} direction(s) is too short to take a landmark golden in two of them",
            directions.len()
        )
    })?;
    for direction in goldens {
        args.push("--golden-direction".into());
        args.push(direction.into());
    }
    let profile = Profile::of(repo_root, &spec.subject.skeleton)?;
    args.extend(published(clip::BAKE_RULES, &profile));
    args.extend(published(bake_check::BLENDER_RULES, &profile));
    let artifacts = Artifacts::new(repo_root, Stage::Bake.as_str(), &spec.name, FIRST_ATTEMPT)?;
    let mut report = blender::run(&script, &args, &artifacts, repo_root)?
        .with_context(|| format!("the bake of {} wrote no report", spec.name))?;
    // Blender measured the action and the scene camera. These read the PNGs it
    // wrote and the one spec field, where no Blender is needed and CI has none.
    let staging = paths.staging();
    let rendered: Vec<bake_check::Rendered<'_>> = names
        .iter()
        .map(|name| bake_check::Rendered {
            name,
            dir: &staging,
            directions,
        })
        .collect();
    report.extend(bake_check::check_files(&rendered, &profile, FIRST_ATTEMPT))?;
    report.write(repo_root)?;
    refuse_off_registry(&report, &profile, &spec.name, "bake")?;
    let mut owed = per_axis(&names);
    owed.extend(bake_check::subjects(&names, &goldens));
    refuse_unreported_subjects(&report, &owed, &spec.name, "bake")?;
    refuse_unread_rules(&report, &bake_rules(), &spec.name, "bake")?;
    anyhow::ensure!(
        !report.has_errors(),
        "the bake of {} left {}, listed in {}",
        spec.name,
        defects(&report),
        artifacts.report().display()
    );

    preview::bake(&names, directions, paths)?;
    preview::strips(&names, paths)?;

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

/// Drops the frames a previous bake left, and only those: packing would pick
/// up a frame of an older shape, and the mesh sent to rigging lives here too.
fn clear_frames(dir: &Path) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // Nothing has been baked here yet.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("reading {}", dir.display())),
    };
    for frame in entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "png"))
    {
        std::fs::remove_file(&frame).with_context(|| format!("clearing {}", frame.display()))?;
    }
    Ok(())
}

/// Nothing but the concept stage retries, so every other report is the first
/// and only attempt.
const FIRST_ATTEMPT: u32 = 1;

/// Where the promotion's diagnostics land. Not a [`Stage`]: it writes shared
/// art rather than a character's, so it has no lock record of its own.
const PROMOTE_STAGE: &str = "promote";

/// How far a joint may move between a character and the rig promoted out of
/// it. Blender imports and re-exports the armature, and a GLB stores a joint
/// position as an `f32`, so this is storage rather than geometry: the
/// committed pair reads 1.02e-5 m, which
/// `the_canonical_rig_carries_the_characters_own_joints` pins.
const PROMOTION_METERS: f64 = 1e-4;

/// Every subject the bake's own two rules own: one per axis of every clip.
fn per_axis(names: &[&str]) -> Vec<String> {
    names
        .iter()
        .flat_map(|name| clip::AXES.map(|axis| format!("{name} {axis}")))
        .collect()
}

/// Every rule the bake boundary owes a finding under: the two the script
/// reports off the pinned copy, and the seven this stage owns.
fn bake_rules() -> Vec<&'static Rule> {
    clip::BAKE_RULES
        .into_iter()
        .chain(bake_check::RULES)
        .collect()
}

/// The variable that turns a golden read into a rewrite, read here and
/// nowhere else. CI asserts it is unset, so a rewritten golden is always a
/// deliberate local act.
pub const UPDATE_GOLDENS_ENV: &str = "MARROWFALL_UPDATE_GOLDENS";

/// Whether this run rewrites its goldens rather than reading them.
fn updating_goldens() -> bool {
    std::env::var_os(UPDATE_GOLDENS_ENV).is_some_and(|value| !value.is_empty())
}

/// The bake parameters the spec fixes, ahead of the per-animation arguments.
fn bake_args(paths: &Paths, spec: &CharacterSpec) -> Vec<OsString> {
    let mut args = vec![
        "--out".into(),
        paths.staging().into(),
        "--goldens".into(),
        paths.goldens().into(),
        "--directions".into(),
        spec.bake.directions.to_string().into(),
        "--size".into(),
        spec.bake.render_size.to_string().into(),
        "--trim-start".into(),
        spec.bake.trim_start.to_string().into(),
    ];
    if updating_goldens() {
        args.push("--update-goldens".into());
    }
    args
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
        pack::seed_import_settings(&dest)?;
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

    check_atlases(spec, &manifest, paths)?;
    preview::sheet(&assets, paths)?;
    // The seeds above are not loadable. Godot writes the imported path the
    // runtime resolves a texture through, and what it writes is what ships.
    godot::import(&paths.godot_project())?;
    Ok(StageRecord {
        note: Some(format!(
            "{} atlases → {}",
            assets.animations.len(),
            paths.relative(&manifest)
        )),
        ..StageRecord::default()
    })
}

/// Measures the packed atlases and their manifest, and refuses a character
/// the game could not draw.
///
/// The pack boundary owns these three, so a manifest is read back through the
/// game's own types by whatever wrote it.
pub fn check_atlases(spec: &CharacterSpec, manifest: &Path, paths: &Paths) -> Result<()> {
    let names: Vec<&str> = spec.animations.iter().map(String::as_str).collect();
    let profile = Profile::of(&paths.root, &spec.subject.skeleton)?;
    let artifacts = Artifacts::new(&paths.root, Stage::Pack.as_str(), &spec.name, FIRST_ATTEMPT)?;
    let mut report = Report::new(Stage::Pack.as_str(), &spec.name, FIRST_ATTEMPT);
    let assets = paths.assets();
    report.extend(atlas::check_files(
        &atlas::Packed {
            manifest,
            dir: &assets,
            animations: &names,
        },
        &profile,
        FIRST_ATTEMPT,
    ))?;
    report.write(&paths.root)?;
    refuse_off_registry(&report, &profile, &spec.name, "pack")?;
    refuse_unread_rules(&report, &atlas::RULES, &spec.name, "pack")?;
    refuse_unreported_subjects(
        &report,
        &atlas::subjects(manifest, &names),
        &spec.name,
        "pack",
    )?;
    anyhow::ensure!(
        !report.has_errors(),
        "the pack of {} left {}, listed in {}",
        spec.name,
        defects(&report),
        artifacts.report().display()
    );
    Ok(())
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}
