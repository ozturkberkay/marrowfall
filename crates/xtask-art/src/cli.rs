//! The `cargo art` command line: what to run, in what order, and when to stop
//! and ask. Stages themselves live in [`crate::stages`].
//!
//! The pipeline splits at the GLB. Before it is AI generation, expensive and
//! not reproducible, so it is committed. After it is deterministic local work,
//! free to re-run. That boundary is why tweaking a sprite setting never
//! re-spends credits.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::io::{BufRead, Write as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use clap::{Parser, Subcommand};

use crate::check::aim::{self, AimTable};
use crate::check::profile::Profile;
use crate::check::{self, Finding, Report, Severity, Symmetry, mesh, rig};
use crate::library::{AnimationLibrary, LibraryLock, MotionSource};
use crate::lock::{Lock, Provider, Stage, StageRecord, TaskRef};
use crate::providers::mixamo::{self, session};
use crate::spec::{CharacterSpec, CharacterType, Paths, View};

/// How long to wait for a browser login before giving up and saying so.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(120);

/// Nothing here retries, so every finding is from the first attempt.
const FIRST_ATTEMPT: u32 = 1;

/// How many concept generations one run may pay for: the first, plus two
/// regenerations. The cap lives here so no uncalibrated gate can spend more.
const CONCEPT_ATTEMPTS: u32 = 3;

/// What all three cost together, at four OpenAI images and about 0.80 USD
/// each. Quoted once, before the loop.
const CONCEPT_DOLLARS: f64 = 2.40;

#[derive(Debug, Parser)]
#[command(name = "cargo art", about = "Marrowfall character art pipeline")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Scaffold a new character spec with the project's locked defaults.
    New {
        name: String,
        #[arg(long, value_enum, default_value_t = CharacterType::Humanoid)]
        kind: CharacterType,
    },
    /// Run the pipeline, resuming from wherever it left off.
    Run {
        name: String,
        /// Start here, discarding this stage and everything after it.
        #[arg(long)]
        from: Option<Stage>,
        /// Run only this stage.
        #[arg(long, conflicts_with = "from")]
        only: Option<Stage>,
        /// Re-run stages already recorded as complete.
        #[arg(long)]
        retry: bool,
        /// Do not pause for review between stages.
        #[arg(long)]
        yes: bool,
    },
    /// Fetch shared motion the library declares but has no file for.
    ///
    /// Separate from `run` because the library is global while a run is one
    /// character, and because the first fetch needs a human to log in.
    Fetch {
        /// Defaults to every entry whose GLB is missing.
        names: Vec<String>,
        /// Fetch again even when the file is already there.
        #[arg(long)]
        force: bool,
    },
    /// Show which stages are complete.
    Status {
        name: String,
        /// Emit machine-readable JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Validate specs and measure the art on disk, without running anything.
    Check {
        /// Defaults to every spec in art/characters.
        name: Option<String>,
        /// Print every rule id, limit, comparison, unit and space, and stop.
        #[arg(long, conflicts_with = "name")]
        list_rules: bool,
    },
}

/// What the driver decided to do with a stage, before any of it happens.
/// Separate from execution so the decision is testable without network or
/// disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Run(Stage),
    /// Already done with matching inputs.
    Cached(Stage),
    /// Not applicable to this character.
    Skipped(Stage, &'static str),
    /// Would re-run a completed stage that costs money; needs confirmation.
    ConfirmSpend(Stage),
}

/// What `cargo art fetch` decided to do with one library entry, before
/// anything is downloaded. Separate from execution so the decision is
/// testable with no network and no browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchStep {
    /// Download it, fit it to the canonical rig, and record it.
    Fetch { name: String, product_id: String },
    /// The file on disk is the one the lock recorded.
    Cached(String),
    /// Not this command's business: bought with a rig, or hand-authored.
    Skipped(String, &'static str),
    /// On disk, but not the file the lock recorded. Reported, never
    /// overwritten: someone put it there on purpose.
    Changed(String),
}

/// Decides what to do with every requested animation. Pure: no IO.
///
/// `on_disk` is the fingerprint of each animation GLB that exists, which is
/// what tells a finished fetch apart from a file edited by hand.
pub fn fetch_plan(
    library: &AnimationLibrary,
    lock: &LibraryLock,
    names: &[String],
    on_disk: &BTreeMap<String, String>,
    force: bool,
) -> Result<Vec<FetchStep>> {
    let requested: Vec<&str> = if names.is_empty() {
        library.animations.keys().map(String::as_str).collect()
    } else {
        names.iter().map(String::as_str).collect()
    };

    requested
        .into_iter()
        .map(|name| {
            let product_id = match &library.get(name)?.source {
                MotionSource::Meshy { .. } => {
                    return Ok(FetchStep::Skipped(
                        name.to_owned(),
                        "arrives with the rig stage",
                    ));
                }
                MotionSource::Authored => {
                    return Ok(FetchStep::Skipped(
                        name.to_owned(),
                        "authored, committed with the art",
                    ));
                }
                MotionSource::Mixamo { product_id } => product_id.clone(),
            };
            let fetch = FetchStep::Fetch {
                name: name.to_owned(),
                product_id,
            };
            if force {
                return Ok(fetch);
            }
            let recorded = lock.fetched.get(name).map(|fetched| fetched.glb.as_str());
            Ok(match on_disk.get(name) {
                None => fetch,
                Some(hash) if recorded == Some(hash.as_str()) => FetchStep::Cached(name.to_owned()),
                Some(_) => FetchStep::Changed(name.to_owned()),
            })
        })
        .collect()
}

/// Options affecting which stages run.
#[derive(Debug, Clone, Copy, Default)]
pub struct RunOptions {
    pub from: Option<Stage>,
    pub only: Option<Stage>,
    pub retry: bool,
}

/// Decides what to do with every stage. Pure: no IO, no side effects. A GLB
/// present without a lock record came from outside this tool and wins, since
/// AI generation is not reproducible.
pub fn plan(
    lock: &Lock,
    spec: &CharacterSpec,
    library: &AnimationLibrary,
    options: RunOptions,
    checkpoint_on_disk: bool,
) -> Vec<Step> {
    // Completion is judged *before* `--from` invalidates anything, so an
    // explicit `--from concept` still warns before re-spending on a stage that
    // had already succeeded.
    let complete: BTreeSet<Stage> = Stage::all()
        .into_iter()
        .filter(|stage| lock.is_current(*stage, spec, library))
        .collect();

    let selected: Vec<Stage> = match (options.only, options.from) {
        (Some(only), _) => vec![only],
        (None, Some(from)) => Stage::all().into_iter().filter(|s| *s >= from).collect(),
        (None, None) => Stage::all().to_vec(),
    };

    selected
        .into_iter()
        .map(|stage| {
            if !spec.subject.kind.can_be_rigged() && stage == Stage::Rig {
                return Step::Skipped(stage, "body plan cannot be auto-rigged");
            }
            let done = complete.contains(&stage);
            // `--from` and `--only` both mean "do this again", so they must
            // not silently reuse the cache, but they must still confirm
            // before spending.
            let explicitly_forced =
                options.retry || options.only == Some(stage) || options.from.is_some();
            if checkpoint_on_disk && !done && !explicitly_forced && stage <= Stage::Download {
                return Step::Skipped(stage, "checkpoint GLB already on disk");
            }
            match (done, explicitly_forced, stage.costs_credits()) {
                (true, false, _) => Step::Cached(stage),
                (true, true, true) => Step::ConfirmSpend(stage),
                _ => Step::Run(stage),
            }
        })
        .collect()
}

/// Locates the workspace root. `CARGO_MANIFEST_DIR` covers the cargo alias;
/// the upward walk covers a directly executed binary.
pub fn repo_root() -> Result<PathBuf> {
    if let Some(manifest) = std::env::var_os("CARGO_MANIFEST_DIR") {
        let dir = PathBuf::from(manifest);
        if let Some(root) = dir.ancestors().find(|dir| dir.join("crates").is_dir()) {
            return Ok(root.to_path_buf());
        }
    }
    let mut dir = std::env::current_dir()?;
    loop {
        if dir.join("Cargo.toml").exists() && dir.join("crates").is_dir() {
            return Ok(dir);
        }
        if !dir.pop() {
            bail!("could not find the workspace root (no Cargo.toml with crates/ above cwd)");
        }
    }
}

pub fn new_character(root: &Path, name: &str, kind: CharacterType) -> Result<()> {
    let paths = Paths::new(root, name);
    if paths.spec().exists() {
        bail!("{} already exists", paths.spec().display());
    }
    CharacterSpec::template(name, kind).save(&paths.spec())?;

    println!("created {}", paths.relative(&paths.spec()));
    println!("\nnext: describe the character in that file, then run:");
    println!("  cargo art run {name}");
    Ok(())
}

pub async fn run(
    root: &Path,
    name: &str,
    options: RunOptions,
    yes: bool,
    input: &mut impl BufRead,
) -> Result<()> {
    let paths = Paths::new(root, name);
    let spec = CharacterSpec::load(&paths.spec())?;
    spec.validate().context("spec is not valid")?;

    let library = AnimationLibrary::load(root)?;
    // Fail before spending anything if a spec names an animation that is not
    // declared; the library's error lists what is available.
    library.resolve(&spec.animations, &spec.subject.skeleton)?;

    let mut lock = Lock::load(&paths.lock())?;
    // Note: the lock is deliberately NOT invalidated up front. `plan` already
    // forces every stage from `--from` onward, and `Lock::record` cascades once
    // a stage actually succeeds, so declining a spend prompt leaves the
    // recorded work, and its task ids, intact.
    let steps = plan(
        &lock,
        &spec,
        &library,
        options,
        paths.character_glb().exists(),
    );

    for step in steps {
        let stage = match step {
            Step::Cached(stage) if lock.is_current(stage, &spec, &library) => {
                println!("{stage}: cached");
                continue;
            }
            // Planned as cached, but an upstream stage has since re-run and
            // invalidated it.
            Step::Cached(stage) => stage,
            Step::Skipped(stage, why) => {
                println!("{stage}: skipped ({why})");
                continue;
            }
            Step::ConfirmSpend(stage) => {
                if !confirm_spend(stage, yes, input)? {
                    println!("{stage}: skipped by user");
                    continue;
                }
                stage
            }
            Step::Run(stage) => stage,
        };

        if let Some(provider) = stage.provider() {
            report_balance(provider).await;
        }
        println!("{stage}: running…");

        let record = match stage {
            Stage::Concept => {
                concept_until_it_holds(&spec, &paths, root, || {
                    crate::stages::concept(&spec, &paths)
                })
                .await?
            }
            Stage::Model => crate::stages::model(&spec, &paths).await?,
            Stage::Rig => {
                // Rigging plus N animations is several separate charges, so
                // each task id is persisted the moment it succeeds.
                let known = lock.tasks();
                let mut fresh: Vec<TaskRef> = Vec::new();
                let outcome =
                    crate::stages::rig(&spec, &library, root, &known, |task| fresh.push(task))
                        .await;
                if outcome.is_err() && !fresh.is_empty() {
                    lock.stages
                        .entry(Stage::Rig)
                        .or_default()
                        .tasks
                        .extend(fresh);
                    lock.save(&paths.lock())?;
                    eprintln!(
                        "  note: {} completed task(s) recorded; re-running will reuse them",
                        lock.stages[&Stage::Rig].tasks.len()
                    );
                }
                outcome?
            }
            Stage::Download => {
                crate::stages::download(&library, &paths, root, &lock.tasks()).await?
            }
            Stage::Bake => crate::stages::bake(&spec, &library, &paths, root)?,
            Stage::Pack => crate::stages::pack(&spec, &library, &paths)?,
        };

        if let Some(note) = &record.note {
            println!("  {note}");
        }
        lock.record(stage, &spec, &library, record);
        lock.save(&paths.lock())?;
    }

    println!("\n{name}: done");
    Ok(())
}

/// Generates the concept views and gates them, three times at most, and
/// wraps nothing else. A Meshy stage is never wrapped: regenerating a mesh
/// costs credits a second generation is no more likely to earn back.
///
/// **Attempt one regenerates too.** Nothing on disk is reused by any of the
/// three, because a set already there is either one a previous run left
/// failing or one this run was asked to replace. [`plan`] is what decides
/// whether this runs at all.
pub async fn concept_until_it_holds<F, Fut>(
    spec: &CharacterSpec,
    paths: &Paths,
    root: &Path,
    mut generate: F,
) -> Result<StageRecord>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<StageRecord>>,
{
    let mut filed: Vec<String> = Vec::new();
    for attempt in 1..=CONCEPT_ATTEMPTS {
        println!("  attempt {attempt} of {CONCEPT_ATTEMPTS}…");
        let record = generate().await?;
        let report = crate::stages::check_concept(spec, paths, root, attempt)?;
        let written = report.artifacts(root)?.report();
        filed.push(paths.relative(&written));
        printed(paths, &report, &written);
        if !report.has_errors() {
            return Ok(record);
        }
    }
    bail!(
        "the concept views of {} failed their gates on all {CONCEPT_ATTEMPTS} attempts, \
         listed in {}. The images are still on disk, so nothing has to be regenerated \
         to look at them",
        spec.name,
        filed.join(", ")
    )
}

/// Asks before re-running a stage that has already been paid for. No terminal
/// means no confirmation.
pub fn confirm_spend(stage: Stage, yes: bool, input: &mut impl BufRead) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    print!("{}", spend_prompt(stage));
    std::io::stdout().flush()?;
    let mut answer = String::new();
    input.read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// What [`confirm_spend`] asks with. The concept stage retries, so its quote
/// is the whole loop rather than one generation, and it is asked once.
pub fn spend_prompt(stage: Stage) -> String {
    let cost = match stage {
        Stage::Concept => format!(
            "up to {CONCEPT_ATTEMPTS} attempts of 4 OpenAI images each, about \
             {CONCEPT_DOLLARS:.2} USD in total"
        ),
        _ => "credits".to_owned(),
    };
    format!("  {stage} already completed and costs {cost}. Re-run? [y/N] ")
}

/// Prints the remaining balance for the provider about to be billed.
/// Advisory: a failed lookup must not stop the pipeline.
pub async fn report_balance(provider: Provider) {
    if provider != Provider::Meshy {
        // OpenAI exposes no balance endpoint on the images API.
        return;
    }
    if let Ok(client) = crate::providers::meshy::Client::from_env()
        && let Ok(balance) = client.balance().await
    {
        println!("  meshy balance: {balance} credits");
    }
}

/// Downloads the motion the library declares but does not have, fits each
/// clip to the canonical rig, and records what it produced.
pub async fn fetch(root: &Path, names: &[String], force: bool) -> Result<()> {
    let library = AnimationLibrary::load(root)?;
    let mut lock = LibraryLock::load(root)?;
    let steps = fetch_plan(&library, &lock, names, &glb_digests(&library, root)?, force)?;

    let mut wanted = Vec::new();
    for step in steps {
        match step {
            FetchStep::Fetch { name, product_id } => wanted.push((name, product_id)),
            FetchStep::Cached(name) => println!("{name}: already fetched"),
            FetchStep::Skipped(name, why) => println!("{name}: skipped ({why})"),
            FetchStep::Changed(name) => println!(
                "{name}: the file on disk is not the one recorded, left alone (--force to replace)"
            ),
        }
    }
    if wanted.is_empty() {
        println!("nothing to fetch");
        return Ok(());
    }

    // Asked for only once something actually needs fetching, so a no-op run
    // never opens a browser.
    let token = mixamo_session()?;
    let client = mixamo::Client::new()?;
    // One at a time: Mixamo rate limits, and a clip takes seconds.
    for (name, product_id) in wanted {
        println!("{name}: fetching {product_id}…");
        let animation = library.get(&name)?;
        let fbx = client
            .motion_fbx(&product_id, animation.source_fps, &token)
            .await?;

        let download = AnimationLibrary::staged_download(root, &name);
        std::fs::create_dir_all(download.parent().unwrap_or(root))
            .with_context(|| format!("creating {}", download.display()))?;
        std::fs::write(&download, &fbx)
            .with_context(|| format!("writing {}", download.display()))?;
        // Before the retarget: the vendor's own rest geometry is on record
        // from here, and nothing downstream still carries it.
        crate::stages::check_source(&download, &name, animation, root)?;
        let glb = library.glb(root, &name);
        crate::stages::retarget(&download, &glb, &name, animation, root)?;

        let fitted = std::fs::read(&glb).with_context(|| {
            format!(
                "the retarget wrote no {}, so nothing was fetched",
                glb.display()
            )
        })?;
        lock.record(&name, animation.source.clone(), &fbx, &fitted);
        lock.save(root)?;
        println!("  → {}", glb.display());
    }
    Ok(())
}

/// Fingerprints of the animation GLBs already on disk.
fn glb_digests(library: &AnimationLibrary, root: &Path) -> Result<BTreeMap<String, String>> {
    let mut digests = BTreeMap::new();
    for name in library.animations.keys() {
        let glb = library.glb(root, name);
        if glb.exists() {
            let bytes =
                std::fs::read(&glb).with_context(|| format!("reading {}", glb.display()))?;
            digests.insert(name.clone(), crate::lock::digest(&bytes));
        }
    }
    Ok(digests)
}

/// The Mixamo credential, asking the user to log in when there is none.
fn mixamo_session() -> Result<session::Token> {
    if let Some(token) = session::token()? {
        return Ok(token);
    }
    let profiles = session::profiles_dir()?;
    println!("Mixamo needs a logged-in session, and there is none.");
    println!(
        "Log in at {} in the Chrome window opening now; this carries on by itself.",
        mixamo::SITE_URL
    );
    session::open_login()?;
    session::wait_for_token(&profiles, LOGIN_TIMEOUT)
}

pub fn status(root: &Path, name: &str, json: bool) -> Result<()> {
    let paths = Paths::new(root, name);
    let spec = CharacterSpec::load(&paths.spec())?;
    let lock = Lock::load(&paths.lock())?;
    let library = AnimationLibrary::load(root)?;

    let state = |stage: Stage| {
        if lock.is_current(stage, &spec, &library) {
            "done"
        } else if lock.stages.contains_key(&stage) {
            "stale"
        } else {
            "todo"
        }
    };

    if json {
        let stages: serde_json::Map<String, serde_json::Value> = Stage::all()
            .into_iter()
            .map(|stage| {
                (
                    stage.as_str().to_owned(),
                    serde_json::json!({
                        "state": state(stage),
                        "note": lock.stages.get(&stage).and_then(|r| r.note.clone()),
                    }),
                )
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "name": name,
                "kind": format!("{:?}", spec.subject.kind),
                "stages": stages,
            }))?
        );
        return Ok(());
    }

    println!("{name} ({:?})", spec.subject.kind);
    for stage in Stage::all() {
        let note = lock
            .stages
            .get(&stage)
            .and_then(|record| record.note.clone())
            .unwrap_or_default();
        println!("  {:<9} {:<5} {note}", stage.as_str(), state(stage));
    }
    Ok(())
}

/// Validates every spec, then measures the concept views, the rig and the
/// mesh each one has on disk.
///
/// A stage whose art is not built yet is named and counted as unbuilt, never
/// as passing. A missing file is an error at the stage boundary, where the
/// stage has just written it; here there is nothing to have written it yet.
pub fn check(root: &Path, name: Option<&str>, list_rules: bool) -> Result<()> {
    if list_rules {
        return print_rule_list(root);
    }
    let dir = root.join("art/characters");
    let specs: Vec<PathBuf> = match name {
        Some(name) => vec![Paths::new(root, name).spec()],
        None => {
            if !dir.exists() {
                println!("no characters yet");
                return Ok(());
            }
            // One directory per character, each holding a spec.ron.
            let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
                .with_context(|| format!("reading {}", dir.display()))?
                .filter_map(Result::ok)
                .map(|entry| entry.path().join("spec.ron"))
                .filter(|path| path.is_file())
                .collect();
            found.sort();
            found
        }
    };

    let mut failed = 0;
    let mut defects = 0;
    let mut unbuilt = 0;
    for path in &specs {
        let spec = match CharacterSpec::load(path).and_then(|spec| {
            spec.validate()?;
            Ok(spec)
        }) {
            Ok(spec) => {
                println!("ok    {}", path.display());
                spec
            }
            Err(error) => {
                failed += 1;
                println!("FAIL  {}: {error:#}", path.display());
                continue;
            }
        };
        for measured in [
            check_concept(root, &spec)?,
            check_rig(root, &spec)?,
            check_mesh(root, &spec)?,
            check_cleaned(root, &spec)?,
            check_cleanup(root, &spec)?,
        ] {
            match measured {
                Some(count) => defects += count,
                None => unbuilt += 1,
            }
        }
    }
    anyhow::ensure!(failed == 0, "{failed} spec(s) invalid");
    println!("\n{} spec(s) ok, {defects} defect(s)", specs.len());
    if unbuilt > 0 {
        println!("{unbuilt} stage(s) have no art on disk yet");
    }
    anyhow::ensure!(defects == 0, "{defects} defect(s)");
    Ok(())
}

/// Measures one character's concept views. `None` when none of the four is
/// on disk: the concept stage is what generates them.
///
/// Through the same producer the retry loop uses, so the mirror and the gate
/// write the same rule set to the same stage name.
fn check_concept(root: &Path, spec: &CharacterSpec) -> Result<Option<usize>> {
    let paths = Paths::new(root, &spec.name);
    if View::ALL
        .into_iter()
        .all(|view| !paths.concept(view).exists())
    {
        println!(
            "      no concept views yet at {}",
            paths.relative(&paths.concept(View::Front))
        );
        return Ok(None);
    }
    let report = crate::stages::check_concept(spec, &paths, root, FIRST_ATTEMPT)?;
    let written = report.artifacts(root)?.report();
    Ok(Some(printed(&paths, &report, &written)))
}

/// Measures one character's rigged GLB against its skeleton profile.
/// `None` when the rig is not on disk.
fn check_rig(root: &Path, spec: &CharacterSpec) -> Result<Option<usize>> {
    let paths = Paths::new(root, &spec.name);
    let glb = paths.character_glb();
    if !spec.subject.kind.can_be_rigged() {
        println!(
            "      no rig: {:?} characters are not rigged",
            spec.subject.kind
        );
        return Ok(None);
    }
    if !glb.exists() {
        println!("      no rig yet at {}", paths.relative(&glb));
        return Ok(None);
    }
    let profile = Profile::of(root, &spec.subject.skeleton)?;
    let table = AimTable::of(root, &spec.subject.skeleton)?;
    let findings = [
        rig::check_file(
            &glb,
            root,
            &profile,
            f64::from(spec.subject.height_meters),
            Symmetry::declared(spec.subject.symmetry),
            FIRST_ATTEMPT,
        )?,
        // The rig stage writes our own rig, so it is named in the canonical
        // convention. A source rig is measured against the same table under
        // its own one.
        aim::check_file(
            &glb,
            root,
            &profile,
            &table,
            table.canonical(),
            FIRST_ATTEMPT,
        )?,
    ]
    .concat();
    report_on(root, &paths, rig::STAGE, findings).map(Some)
}

/// Measures one character's bare mesh, which is the mesh before rigging.
/// `None` when it is not on disk: the `model` stage downloads it.
fn check_mesh(root: &Path, spec: &CharacterSpec) -> Result<Option<usize>> {
    let paths = Paths::new(root, &spec.name);
    let glb = paths.bare_glb();
    if !glb.exists() {
        println!("      no bare mesh yet at {}", paths.relative(&glb));
        return Ok(None);
    }
    let findings = file_rules(root, spec, &glb)?;
    report_on(root, &paths, mesh::STAGE, findings).map(Some)
}

/// Measures the mesh the fixer wrote against the same rules, because that is
/// the file rigging is sent. `None` when no fixer was asked for, or when one
/// was and has not run yet: the rig stage is what runs it.
fn check_cleaned(root: &Path, spec: &CharacterSpec) -> Result<Option<usize>> {
    let paths = Paths::new(root, &spec.name);
    let glb = paths.clean_glb();
    if !spec.subject.cleanup {
        println!("      no cleaned mesh: spec.subject.cleanup is false");
        return Ok(None);
    }
    if !glb.exists() {
        println!("      no cleaned mesh yet at {}", paths.relative(&glb));
        return Ok(None);
    }
    let findings = mesh::only(&mesh::CLEANED_RULES, file_rules(root, spec, &glb)?);
    report_on(root, &paths, mesh::CLEANED_STAGE, findings).map(Some)
}

/// Measures what the fixer wrote against what it was given. `None` when
/// there is no pair to read: no mesh at all, or a cleanup that has not run.
fn check_cleanup(root: &Path, spec: &CharacterSpec) -> Result<Option<usize>> {
    let paths = Paths::new(root, &spec.name);
    let (bare, clean) = (paths.bare_glb(), paths.clean_glb());
    for file in [Some(&bare), spec.subject.cleanup.then_some(&clean)]
        .into_iter()
        .flatten()
    {
        if !file.exists() {
            println!("      no cleanup yet, no {}", paths.relative(file));
            return Ok(None);
        }
    }
    let profile = Profile::of(root, &spec.subject.skeleton)?;
    let findings = mesh::check_cleanup_files(
        &bare,
        spec.subject.cleanup.then_some(clean.as_path()),
        root,
        &profile,
        FIRST_ATTEMPT,
    );
    report_on(root, &paths, mesh::CLEANUP_STAGE, findings).map(Some)
}

/// Every file rule on one mesh, at the height and the symmetry the spec
/// declares.
fn file_rules(root: &Path, spec: &CharacterSpec, glb: &Path) -> Result<Vec<Finding>> {
    let profile = Profile::of(root, &spec.subject.skeleton)?;
    mesh::check_file(
        glb,
        root,
        &profile,
        f64::from(spec.subject.height_meters),
        Symmetry::declared(spec.subject.symmetry),
        // `check` measures what is on disk and calls nothing, so
        // `mesh.printability` reports as unavailable. The model stage is
        // where the response comes from.
        None,
        FIRST_ATTEMPT,
    )
}

/// Prints the defects of one stage's findings and writes its report.
fn report_on(root: &Path, paths: &Paths, stage: &str, findings: Vec<Finding>) -> Result<usize> {
    let mut report = Report::new(stage, &paths.name, FIRST_ATTEMPT);
    report.extend(findings)?;
    let written = report.write(root)?;
    Ok(printed(paths, &report, &written))
}

/// One written report on the terminal, and how many defects it holds.
///
/// The passing measurements stay in the report; the terminal gets the defects
/// and anything a declaration switched off.
fn printed(paths: &Paths, report: &Report, written: &Path) -> usize {
    for finding in report.findings() {
        if finding.severity != Severity::Info {
            println!("  {}", defect(finding));
        }
    }
    let defects = report
        .findings()
        .iter()
        .filter(|finding| finding.severity == Severity::Error)
        .count();
    println!(
        "      {}: {} measurement(s), {defects} defect(s), report at {}",
        report.stage(),
        report.findings().len(),
        paths.relative(written)
    );
    defects
}

/// One itemized defect: what was measured, what was allowed, and why it
/// matters, on one line.
fn defect(finding: &Finding) -> String {
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

/// Prints every rule with the limit each skeleton's profile publishes for
/// it. Every skeleton, because the limits are per profile and a second one
/// is a second file rather than a code change.
fn print_rule_list(root: &Path) -> Result<()> {
    let skeletons = Profile::declared(root)?;
    anyhow::ensure!(
        !skeletons.is_empty(),
        "no skeleton profile in {}",
        Profile::dir(root).display()
    );
    // Widest id and widest unit, so a new rule cannot push the columns out of
    // line.
    let widest = |of: fn(&&check::Rule) -> usize| {
        check::every_rule()
            .map(|rule| of(&rule))
            .max()
            .unwrap_or_default()
    };
    let (width, units) = (widest(|rule| rule.id.len()), widest(|rule| rule.unit.len()));
    for skeleton in skeletons {
        let profile = Profile::of(root, &skeleton)?;
        println!("rules for the {skeleton:?} skeleton, from its [profile]\n");
        for rule in check::every_rule() {
            println!(
                "{:<width$} {} {:<8} {:<units$} {}",
                rule.id,
                rule.comparison.as_str(),
                rule.printed_limit(&profile),
                rule.unit,
                rule.space,
            );
        }
        println!();
    }
    Ok(())
}

/// Parses the command line and dispatches. The binary is only a call to this,
/// so every path stays reachable from the tests.
pub async fn run_from_args<I, T>(argv: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    // `--help` and `--version` arrive as clap "errors". They are not failures:
    // clap renders them itself and exits 0. Anything else is a real parse
    // error, returned so the caller (and the tests) can see it.
    let cli = Cli::try_parse_from(argv).map_err(|error| match error.kind() {
        clap::error::ErrorKind::DisplayHelp
        | clap::error::ErrorKind::DisplayVersion
        | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => error.exit(),
        _ => anyhow::Error::from(error),
    })?;
    let root = repo_root()?;

    match cli.command {
        Command::New { name, kind } => new_character(&root, &name, kind),
        Command::Run {
            name,
            from,
            only,
            retry,
            yes,
        } => {
            run(
                &root,
                &name,
                RunOptions { from, only, retry },
                yes,
                &mut std::io::stdin().lock(),
            )
            .await
        }
        Command::Fetch { names, force } => fetch(&root, &names, force).await,
        Command::Status { name, json } => status(&root, &name, json),
        Command::Check { name, list_rules } => check(&root, name.as_deref(), list_rules),
    }
}
