//! The `pose_mode` spike: the same four concept views reconstructed three
//! times, unset, `a-pose` and `t-pose`, and every `mesh.*` and `rig.*` rule
//! read on each.
//!
//! Not a stage. Nothing here writes the lock or touches committed art: each
//! mode gets its own directory under `art/staging/<char>/spike/`, and what a
//! human reads is the report per mode plus the contact sheet beside it.
//!
//! Step 0 is free. `pose_mode` is documented for Multi-Image to 3D only by
//! inheritance, so before any credit is spent the driver sends deliberately
//! invalid bodies and reads what the API says about the field.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde_json::json;

use crate::blender::{self, BLENDER_SRC};
use crate::check::{Artifacts, Finding, Report, Severity, rig};
use crate::preview;
use crate::providers::meshy::{self, Client, Endpoint};
use crate::spec::{CharacterSpec, Paths, PoseMode};
use crate::stages;

/// Nothing here retries, so every finding is from the first attempt.
const FIRST_ATTEMPT: u32 = 1;

/// What one generation costs, and what one rigging call costs, quoted before
/// each so nobody is surprised by a bill.
const MODEL_CREDITS: u32 = 30;
const RIG_CREDITS: u32 = 5;

/// Render size of one contact-sheet cell. Large enough that a forearm's top
/// surface is legible at the close view.
const SHEET_PIXELS: u32 = 512;

/// An `image_urls` entry no reconstructor can fetch, so step 0's request is
/// refused before it can become a task.
const NOT_AN_IMAGE: &str = "not-a-url";

/// A `pose_mode` nobody documents, which is how the probe tells a field the
/// API knows from one it ignores.
const NOT_A_POSE: &str = "banana";

/// Where one mode's derived files live, under the character's own staging
/// directory. Public so a test can pin the naming without a network: three
/// modes sharing one directory would be three runs overwriting each other.
pub fn paths_for(paths: &Paths, mode: Option<PoseMode>) -> Paths {
    let named = PoseMode::named(mode);
    paths.variant(Path::new("spike").join(named), named)
}

/// The contact sheet a human reads to answer acceptance item 4.
fn sheet_png(paths: &Paths) -> PathBuf {
    paths.staging().join("sheet.png")
}

/// Step 0, free: whether the API knows `pose_mode` at all.
///
/// Three requests, none of which can become a task. The first is the
/// baseline: an identical refusal means the field is accepted, and a value
/// nobody documents refused by name means it is validated too.
pub async fn probe_pose_mode(client: &Client) -> Result<()> {
    let body = |pose: Option<&str>| {
        let mut body = json!({
            "image_urls": [NOT_AN_IMAGE],
            "ai_model": "meshy-7",
        });
        if let Some(pose) = pose {
            body["pose_mode"] = json!(pose);
        }
        body
    };
    for (what, pose) in [
        ("no pose_mode", None),
        ("pose_mode a-pose", Some(PoseMode::APose.as_str())),
        ("pose_mode banana", Some(NOT_A_POSE)),
    ] {
        let (status, said) = client.probe(Endpoint::MultiImageTo3d, body(pose)).await?;
        println!("  step 0, {what}: {status} {}", said.trim());
    }
    Ok(())
}

/// Generates one mode's mesh, cleans it, measures both and draws the sheet.
/// A bare mesh already on disk is reused: it cost 30 credits.
///
/// **A failing verdict does not stop this, and it stops the rig stage.** The
/// stage refuses because rigging a failing mesh wastes credits. Here the
/// limits are what is being measured, so [`run`] carries the verdict instead.
async fn build(
    spec: &CharacterSpec,
    paths: &Paths,
    root: &Path,
    client: &Client,
    mode: Option<PoseMode>,
) -> Result<(PathBuf, Option<String>)> {
    let bare = paths.bare_glb();
    if bare.exists() {
        println!("  reusing {}", paths.relative(&bare));
    } else {
        println!(
            "  balance {} credits, about to spend {MODEL_CREDITS}",
            client.balance().await?
        );
        generate(spec, paths, mode).await?;
    }
    draw_sheet(paths, root)?;

    // All three reports are on disk either way: `clean_mesh` writes them
    // before it refuses any of them.
    match stages::clean_mesh(spec, paths, root) {
        Ok(mesh) => Ok((mesh, None)),
        // The fixer wrote `clean.glb` before the gates were read, so the file
        // rigging would be handed is on disk whatever the verdict.
        Err(why) => {
            let mesh = if spec.subject.cleanup {
                paths.clean_glb()
            } else {
                bare
            };
            anyhow::ensure!(mesh.exists(), "no mesh to measure: {why:#}");
            Ok((mesh, Some(format!("{why:#}"))))
        }
    }
}

/// One paid reconstruction, into this mode's own directory.
///
/// The model stage itself, asked for a rest pose: a spike that sent its own
/// request would be measuring a body the pipeline never builds. Its record
/// goes nowhere, because nothing here writes the lock.
async fn generate(spec: &CharacterSpec, paths: &Paths, mode: Option<PoseMode>) -> Result<()> {
    let mut asked = spec.clone();
    asked.subject.pose_mode = mode;
    stages::model(&asked, paths).await.map(drop)
}

/// The five views of the bare mesh, composited into one sheet.
fn draw_sheet(paths: &Paths, root: &Path) -> Result<()> {
    let script = root.join(BLENDER_SRC).join("mesh_sheet.py");
    let views = paths.staging().join("views");
    let artifacts = Artifacts::new(root, "sheet", paths.item(), FIRST_ATTEMPT)?;
    let args = vec![
        OsString::from("--glb"),
        paths.bare_glb().into(),
        OsString::from("--out-dir"),
        views.clone().into(),
        OsString::from("--size"),
        SHEET_PIXELS.to_string().into(),
    ];
    let findings = blender::run(&script, &args, &artifacts, root)
        .with_context(|| format!("drawing {}", paths.relative(&sheet_png(paths))))?;
    anyhow::ensure!(
        findings.is_none(),
        "the sheet script wrote a report, and it measures nothing: a human \
         reads the pixels"
    );

    let cells: Vec<image::RgbaImage> = ["front", "back", "left", "right", "forearms"]
        .iter()
        .map(|view| {
            let file = views.join(format!("{view}.png"));
            image::open(&file)
                .map(|image| image.to_rgba8())
                .with_context(|| format!("reading {}", file.display()))
        })
        .collect::<Result<_>>()?;
    preview::write_grid(&cells, cells.len() as u32, &sheet_png(paths))
}

/// One paid rigging call on a cleaned mesh, and every `rig.*` rule on what
/// came back. The animation library is not touched: a spike buys a skeleton
/// to measure, never motion.
async fn attach_rig(
    spec: &CharacterSpec,
    paths: &Paths,
    root: &Path,
    client: &Client,
    mesh: &Path,
) -> Result<Report> {
    let rigged = paths.rigged_glb();
    if rigged.exists() {
        println!("  reusing {}", paths.relative(&rigged));
    } else {
        println!(
            "  balance {} credits, about to spend {RIG_CREDITS}",
            client.balance().await?
        );
        let glb = std::fs::read(mesh).with_context(|| format!("reading {}", mesh.display()))?;
        let submitted = Artifacts::new(root, rig::STAGE, paths.item(), FIRST_ATTEMPT)?;
        let task = client
            .run(
                Endpoint::Rigging,
                meshy::rigging_body(&meshy::to_model_uri(&glb), spec.subject.height_meters),
                &submitted.task(),
                |progress| println!("  rig {progress}%"),
            )
            .await?;
        println!("  rig task {}, {:?} credits", task.id, task.credits);
        let url = task
            .glb_url()
            .context("the finished rig task exposes no GLB url")?;
        client.download(url, &rigged).await?;
    }

    // Through the step's own producer, so a mode is measured in the
    // convention its own bones are named in rather than in ours.
    let bytes = std::fs::read(&rigged).with_context(|| format!("reading {}", rigged.display()))?;
    let report = stages::check_rig(spec, paths, root, rig::STAGE, &rigged, &bytes)?;
    let written = report.artifacts(root)?.report();
    for finding in report.findings() {
        if finding.severity != Severity::Info {
            println!("  {}", one_line(finding));
        }
    }
    println!("  rig report at {}", paths.relative(&written));
    Ok(report)
}

/// One finding on one line, the way `cargo art check` prints a defect.
fn one_line(finding: &Finding) -> String {
    format!(
        "{severity} {rule} {subject}: {measured:.3} {comparison} {limit} {unit}. {message}",
        severity = match finding.severity {
            Severity::Error => "ERROR",
            Severity::Warning => "WARN ",
            Severity::Info => "info ",
            Severity::Skipped => "off  ",
        },
        rule = finding.rule,
        subject = finding.subject,
        measured = finding.measured,
        comparison = finding.comparison.as_str(),
        limit = finding.limit,
        unit = finding.unit,
        message = finding.message,
    )
}

/// What this run would still buy. A mode whose files are on disk is free, so
/// a second run of the whole spike owes nothing.
pub fn outstanding(paths: &Paths, rig: bool) -> u32 {
    PoseMode::ALL
        .into_iter()
        .map(|mode| {
            let mine = paths_for(paths, mode);
            let mesh = u32::from(!mine.bare_glb().exists()) * MODEL_CREDITS;
            let skeleton = u32::from(rig && !mine.rigged_glb().exists()) * RIG_CREDITS;
            mesh + skeleton
        })
        .sum()
}

/// What the spike asks before it bills, quoting what [`outstanding`] read.
pub fn spend_prompt(credits: u32) -> String {
    format!("  spike-pose is about to spend {credits} credits. Continue? [y/N] ")
}

/// Runs the spike end to end and reports what every mode read.
///
/// Free to re-run: a mode whose files are on disk is measured again and
/// bought again never. `rig` is what unlocks the 5 credits per mode.
pub async fn run(
    root: &Path,
    name: &str,
    rig: bool,
    yes: bool,
    input: &mut impl std::io::BufRead,
) -> Result<()> {
    let paths = Paths::new(root, name);
    let spec = CharacterSpec::load(&paths.spec())?;
    spec.validate()?;
    let client = Client::from_env()?;

    let owed = outstanding(&paths, rig);
    if owed > 0 && !crate::cli::confirm_spend(&spend_prompt(owed), yes, input)? {
        println!("nothing spent");
        return Ok(());
    }

    println!("balance before: {} credits", client.balance().await?);
    probe_pose_mode(&client).await?;

    let mut defective = Vec::new();
    for mode in PoseMode::ALL {
        let named = PoseMode::named(mode);
        let mine = paths_for(&paths, mode);
        println!("\n{named}: {}", mine.relative(&mine.staging()));
        let (mesh, verdict) = build(&spec, &mine, root, &client, mode).await?;
        match verdict {
            None => println!("  mesh gates: every rule holds"),
            Some(why) => {
                println!("  mesh gates: {why}");
                defective.push(named);
            }
        }
        println!("  sheet at {}", mine.relative(&sheet_png(&mine)));
        if rig {
            attach_rig(&spec, &mine, root, &client, &mesh).await?;
        }
    }

    println!("\nbalance after: {} credits", client.balance().await?);
    anyhow::ensure!(
        defective.is_empty(),
        "the mesh gates reject {}. Every reading is in the reports above: \
         either that mode is not the one to adopt, or [profile.mesh] is due a \
         recalibration on what it measured",
        defective.join(", ")
    );
    Ok(())
}
