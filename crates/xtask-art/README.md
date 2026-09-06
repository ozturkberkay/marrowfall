# Art pipeline

`cargo art` takes a character from a written description to sprite atlases the
game can load.

```sh
cargo art new skeleton --kind humanoid    # scaffold art/characters/skeleton/spec.ron
# describe the character in that file
cargo art run skeleton                    # run the pipeline, resuming where it left off
cargo art status skeleton                 # what is done, stale or pending
cargo art check                           # validate the specs, measure the art
cargo art check --list-rules              # every gate: limit, comparison, unit, space
```

A run asks nothing except before it re-spends: the one prompt left is
`ConfirmSpend`, on a paid stage that is already recorded as complete.
`--retry` is what re-runs those stages, and `--yes` answers the prompt for an
unattended run. Nothing pauses for a human to look at a stage's output;
the gates below decide, and a pull request is where a human signs off.

`check` reads the art on disk and prints one line per defect, one report per
rule set. It measures the four concept views against the five `concept.*`
rules, then three files per character: the rigged
`art/characters/<name>/model.glb` against the thirteen `rig.*` rules,
`art/staging/<name>/bare.glb`, the mesh before rigging, against the thirteen
`mesh.*` file rules, and `art/staging/<name>/clean.glb`, what the fixer wrote,
against eleven of those thirteen plus the two that read the pair. A file that
is not there yet is reported as unbuilt, never as passing.

The cleaned file drops two of the thirteen. `mesh.non_manifold`'s ceiling is
calibrated before the fixer and filling a hole raises that count on purpose,
so afterwards it is `mesh.non_manifold_post`'s. `mesh.printability` asks Meshy
about a model task, and a file written locally has none.

Today the committed survivor breaks five of the `rig.*` rules, on 14 subjects
between them, so `check` exits non-zero on it. The rig is regenerated later in
the pipeline work, and the rules become required checks then.

The `clip.*` and `source.*` rules run at their own stage boundaries rather
than here, because each one needs something `check` does not have.

The six `source.*` rules run at **fetch** time, on the vendor file as it
arrived and before anything is fitted to it. `source.fps_declared` and the
`source.traveling` / `source.in_place` pair say the clip is the one
`library.ron` declares. `source.wander`, `source.child_axis` and
`source.posture` record what nothing downstream still carries: the vendor
rig's own geometry, and how far the hips got from where they started on the
way. They record rather than gate, because a vendor skeleton is not ours to
regenerate (Mixamo's `Neck` axis sits 16.933 degrees off the direction to its
own `Head`, and it always will) and because an in-place cycle wanders 0.0276 m
against a strafe's 2.3117, which no one threshold reads.

Fourteen `clip.*` rules run at the **retarget** boundary. `clip.swing` and
`clip.twist` measure the delivered GLB against the file its motion was bought
in; that file is an FBX, so `retarget_animation.py` writes the source's own
world orientations to `art/staging/reports/retarget.<clip>.1.source.json` and
`check/clip.rs` reads that beside the GLB it just wrote. `clip.fps_grid` and
`clip.fps_grid.range` are requirement 3: the scene runs at the clip's own
`source_fps`, so a 30 fps clip cannot be read on a 24 fps grid, land at frames
0.8 to 16.8 and lose four of them to rounding. `clip.loop` reads a looping
clip's last pose against its first.

`clip.floor_snap`, `clip.stride` and `clip.stride_ratio` are requirement 4.
The retarget sizes every length by the femur and lifts the clip until its
lowest toe stands where the rig's own rest pose stands. **The floor is not
zero**: on this skeleton the toe joint is the ball of the foot and rests
0.0307 m above the sole. `clip.stride` holds the fit's own travel to the
source's travel sized by that same ratio, within 2 percent, and
`clip.stride_ratio` puts the ratio itself on record. A clip the library
declares in place has no travel to be sized, so `travels` reports
`clip.stride` as `skipped`.

All three are measured **at two sites**, like `clip.fps_grid`: once inside
Blender on the pose it just evaluated, and once here in Rust on the file that
was written from it. The Rust half is what gives them a negative control in
CI, and it re-derives our own stride segment off the rig GLB rather than
trusting the length the retarget wrote, so the two sides of the ratio have two
readers. The source's own travel and femur cannot be re-derived, because the
vendor file is an FBX, so both ride in the sidecar beside its rotations.

`clip.foot_contact.plants`, `.skate` and `.penetration` are requirement 6.
The retarget finds the frames each foot is on the ground, holds it there
through them, and reports how often it planted, how far it slid while it was
down and how far its sole got under the ground plane at zero. All three read a
**sole point** rather than the toe joint; corrections 1 and 2 of T9 in
`docs/design/2026_08_20_art_pipeline_foundations.md` say why. A clip the
library declares in place has a ground that moves under it, so `travels`
reports the first two as `skipped`.

`clip.root_travel` and `clip.root_bob` run at the **bake**, on the copy
`strip_root_motion` has just pinned, because that copy is never written to
disk. The strip pins the two horizontal axes and keeps the vertical one, so
they are two rules with two limits: a residual of 0.02 m and a bob of 0.15.
The bake scales nothing: the retarget already sized every length, so a clip
whose own rig is not this character's size is refused there rather than
rescaled a second time.

## Gating the concept views, and retrying them

Five `concept.*` rules read the four generated PNGs before a single Meshy
credit is spent on them, in Rust, so CI runs them too. They measure the
**silhouette**: the generator returns fully opaque images, so there is no
alpha to separate the figure with, and the figure is instead whatever a flood
fill from the border does not reach within 12 levels of the border's own
median color.

| Rule | Reads |
|---|---|
| `concept.background_flat` | how even the fill behind the figure is, in levels of 0 to 255, over the region the border fill reached |
| `concept.single_figure` | how many pieces of silhouette are big enough to be a figure |
| `concept.arm_gap` | what share of a torso band's rows show both arms clear of the ribcage |
| `concept.mirror` | how far each row's two silhouette edges sit from their reflection, at the 99th percentile of the rows |
| `concept.cross_view` | how far two views disagree about the figure's height and where its weight sits |

`arm_gap` and `mirror` own the front and back views only: a side view shows
the arms in front of the torso and has no left half to read against a right.
`cross_view` owns the six pairs. `symmetry: false` reports `concept.mirror` as
`skipped`, the same declaration `mesh.mirror` reads. Every limit is calibrated
on the four committed views, with the reading and the headroom written beside
it in `art/skeletons/humanoid.toml`.

**A failing view is regenerated, up to three times in total.** The loop lives
in `cli.rs::run` and wraps the concept stage and nothing else:
`ConfirmSpend` quotes about 2.40 USD for up to 3 attempts of 4 images each,
once, before the loop starts. Every attempt writes
`art/staging/reports/concept.<name>.<attempt>.json` and prints what it
measured. After the third failure the run stops with all three reports named
and the images left on disk, so a human can see what the generator kept
getting wrong.

**Attempt one regenerates too, and so does every retry.** The stage reuses
nothing on disk: a new front view invalidates the three derived from it, and a
set already there is either one a previous run left failing or one this run
was asked to replace. The cost of that is the only case it loses on, a run
that died after three of the four views arrived, which pays for all four
again. Whether the stage runs at all is the plan's decision, which reads the
lock, so a completed concept stage is not re-run without `--retry`.

A Meshy stage is never wrapped. Its gates run inside the stage, before the
credits, and a failure stops the run: a mesh generation is expensive and a
second one is no more likely to pass than the first.

## Cleaning the mesh before rigging

Two spec flags decide what happens to the geometry, and both are off unless a
spec says otherwise:

```ron
subject: Subject(
    kind: Humanoid,
    cleanup: true,   # weld, drop debris and fill holes before rigging
    symmetry: true,  # mirror it, and hold the mirror rules to it
),
```

`cleanup` runs `tools/blender/src/mesh_clean.py` between the model stage and
the rigging call, on `bare.glb`, and writes `clean.glb`. It runs there and
nowhere else: `model.glb` is already skinned, and editing geometry under a
skin desyncs the weights. `symmetry` decides whether that fixer mirrors the
mesh, and whether `mesh.mirror`, `rig.mirror_length` and `rig.mirror_direction`
measure or report `skipped` on the declaration. A monster can be asymmetric on
purpose, so this is per character.

Rigging is then sent the mesh **by value**, as a `data:model/gltf-binary`
URI, with no `input_task_id`: that field wins if both are sent, so with it
there Meshy would rig the mesh it generated and everything the fixer did
would be discarded. The survivor's cleaned mesh is 4.19 MB, which is 5.59 MB
of base64. Meshy states no body size limit for `model_url`, so that number is
what the first real call has to check.

The fixer measures nothing. `check/mesh.rs` reads the mesh it was given
beside the mesh it wrote and reports `mesh.cleanup_effective`, which is
strictly less than the defects it started with, and `mesh.non_manifold_post`,
which is the ceiling for what filling a hole leaves behind.

**Today the survivor's cleaned mesh fails `mesh.self_intersect`**, 1026 faces
against a provisional 1000, because mirroring copies the crossings of the half
it keeps. So `cargo art check` exits non-zero and the rig stage refuses to
spend on it. That row is one of the ones the recalibration below is for.

### Still waiting on a Meshy key

Every `[profile.mesh]` limit is calibrated on the rigged `model.glb`, or on a
stand-in lifted out of it, because no key on this machine could download
`bare.glb`. With a working key, in this order and for no credits until the
last step:

```sh
export MESHY_API_KEY=...
# 0 credits: the model task is already paid for, and its GLB is the bare mesh.
id=$(grep -A4 'Model: StageRecord' art/characters/survivor/spec.lock | grep 'id:' | cut -d'"' -f2)
task=https://api.meshy.ai/openapi/v1/multi-image-to-3d/$id
url=$(curl -sH "Authorization: Bearer $MESHY_API_KEY" $task | jq -r .model_urls.glb)
curl -sL "$url" -o art/staging/survivor/bare.glb

cargo art check survivor          # the pre-cleanup set, on the real mesh
```

Then replace every `[profile.mesh]` row marked provisional with what that run
read, plus its published headroom, and drop the marker. Run the fixer and
measure what it wrote:

```sh
cargo art run survivor --only rig   # 5 credits: cleans, then rigs by data URI
cargo art check survivor            # mesh.non_manifold_post, on the real clean.glb
```

`mesh.non_manifold_post` and `mesh.self_intersect` are the two rows that can
only be set from that pair: the first is what filling holes left behind, and
the second is the one the fixer makes worse. The rigging call is the 5 credits,
and it is the first proof that a data URI of this size is accepted at all.

Every mesh rule measures **world space first, then welded**, and says so in
its finding. glTF splits one vertex at every UV seam, so a naive read of the
survivor counts 13,368 boundary edges on a mesh that has 171. The weld
distance is 1e-5 m, chosen from a merge histogram whose plateau runs from
1e-9 m to 1e-4 m. Every `[profile.mesh]` limit is **provisional**: it is
calibrated on the rigged `model.glb`, because `bare.glb` has never been
downloaded.

The pipeline splits at the GLB. Concept art and the 3D model are generated by
paid APIs and cannot be reproduced, so they are committed alongside the spec in
`art/characters/<name>/` as
checkpoints. Everything after, the Blender bake and sprite packing, is
deterministic and free to re-run, so changing a sprite setting never re-spends
credits:

```sh
cargo art run skeleton --from bake        # re-render sprites only
```

Animations are stored the way engines store them, and **shared**. Meshy rigs
every humanoid onto the same 24-bone skeleton, and almost all of a clip is bone
*rotation*, which does not depend on a character's proportions, so a run cycle
bought once drives every later character for free. `art/animations/library.ron`
declares each motion once (its Meshy id, whether it loops); a character spec
lists names:

```ron
animations: ["idle", "run", "run_back"],
```

Ten characters wanting the same five animations therefore cost five purchases,
not fifty.

## Stages

Six stages, fixed in code (`Stage::all`), not declared per character. They are
what `art/characters/<name>.lock` records:

| Stage | Cost | Runs in | Does |
|---|---|---|---|
| `concept` | paid | Rust | Turns the written description into four concept views, gated and retried up to three times. |
| `model` | paid | Rust | Concept art to a textured 3D mesh. |
| `rig` | paid | Rust | Adds a skeleton, then one animation clip per entry in `animations`. |
| `download` | free | Rust | Fetches the finished GLBs and splits them: mesh once, one file per clip. |
| `bake` | free | **Blender** | Renders every clip through 8 compass directions into loose PNG frames. |
| `pack` | free | Rust | Crops and packs those frames into one atlas per clip, plus the manifest Godot reads. |

The `.ron` spec describes the *character*; the `.lock` records *what has been
built*. So `pack` appears in the lock without appearing in the spec, its
settings live in the spec's `bake` block, which is one block of sprite settings
shared by the last two stages rather than one block per stage.

The pipeline is Rust. `tools/blender/src/` holds the only Python in the repo,
because `bpy` is Python-only and Blender is the one tool that cannot be driven
any other way: `bake_sprites.py`, `check_source.py`, `mesh_clean.py`,
`retarget_animation.py` and `strip_animation.py` run inside Blender, and
`cleanup.py`, `clip.py`, `findings.py`, `framing.py`, `skeleton.py`,
`source.py` and `transfer.py` are the `bpy`-free modules they import.
`cargo art` shells out to `blender --background --python …` for those five,
and does everything else itself. Every published limit those scripts report
against is passed to them as `--limit RULE=NUMBER`, read off the same rule
list `--list-rules` prints, so no script holds a second copy of a number.

## Shared animations

Motion is declared once in `art/animations/library.ron` and shared by every
character on the same skeleton. Most of it is committed. What is not, because
its license allows the animation but not republishing the file, is fetched:

```bash
cargo art fetch            # every clip declared but missing
cargo art fetch strafe_left  # just one, --force to replace it
```

A fetch is three steps: the export is downloaded, `check_source.py` measures
it against what the library declares, and only then is it fitted to the
canonical rig. Every export is requested **traveling**, so the root carries
the motion and the femur ratio can size the step to our own body, and at the
rate `source_fps` declares. `source_fps` is the clip's own rate and `fps` is
the sprite sampling rate: `idle` is sampled 8 times a second out of a clip
authored at 24.

The first run opens Chrome at Mixamo and waits while you log in, because
exporting needs a session credential that lasts about a day. Nothing is stored:
the credential is read from the browser each time it is needed.
