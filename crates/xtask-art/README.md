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
cargo art check --sheet                   # draw the contact sheet of the committed atlases
cargo art spike-pose skeleton             # measure which pose_mode to send, ~90 credits
```

A run asks nothing except before it spends. There are two such prompts:
`ConfirmSpend`, on a paid stage that is already recorded as complete, and
`spike-pose`'s own, which quotes the credits it still has to buy. `--retry`,
`--from` and `--only` are what re-run a paid stage, and `--yes` answers
either prompt for an unattended run. Nothing pauses for a human to look
at a stage's output; the gates below decide, and a pull request is where a
human signs off.

`check` reads the art on disk and prints one line per defect, one report per
rule set. It measures the four concept views against the five `concept.*`
rules, then three files per character: the rigged
`art/characters/<name>/model.glb` against the fourteen `rig.*` rules,
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

`rig.elbow_bend` is the one that records rather than gates: it reads how far
each forearm sits out of line with its own upper arm, and every rig this
pipeline has bought carries some of it, 24 degrees on the committed one. A
published limit would fail forever on a rig nothing here can regenerate, and
the reading is what the `pose_mode` spike below needs.

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

## Gating the bake, and the atlas

Seven `bake.*` rules run at the **bake** boundary, beside those two. Five of
them read the PNGs the render left and the one spec field, in Rust, so CI
runs them with no Blender:

| Rule | Reads |
|---|---|
| `bake.frame_count` | whether the rendered set is a full rectangle: one frame per direction per frame, contiguous from zero |
| `bake.non_empty` | what share of its own canvas the emptiest frame covers |
| `bake.in_frame` | how close the tightest frame's content comes to a canvas border |
| `bake.pivot` | how far two opposite directions sit from being each other's reflection about the canvas center |
| `bake.forearm_roll` | whether `spec.bake.forearm_roll` still asks for the patch the world-space transfer replaced |

`bake.pivot` is the one that needs saying. An orthographic camera centered on
the axis the ring turns about maps a point at world `x` to the mirror of where
it maps it half a turn around, so the two content spans of opposite directions
reflect about the middle of the canvas **exactly**, whatever the pose. All 848
frames of the survivor read 0 or 1 px there. The ground line does not work
that way: a 35 degree camera projects depth onto the vertical axis of the
image, so turning the character moves its lowest foot 28 to 86 px up or down
the frame, and that is correct rather than a defect.

The other two need the scene, so `bake_sprites.py` measures them and
`check/bake.rs` only publishes them. `bake.sampled_frames_are_keys` reads the
clip's own action: every frame the bake renders has to be a frame something
keyed, or the sprite shows a pose halfway between two nobody authored. There
is no divisibility rule between `fps` and `source_fps` and none is wanted;
this is the invariant that was actually meant.

`bake.landmark_golden` projects every joint of the rig through the bake camera
to whole pixels and reads them against a committed text golden, three sampled
frames by two directions per clip:

```text
art/goldens/survivor/idle_s.txt
frame  bone                x     y
0      Head              252  153
0      Hips              256  234
```

Two directions, because a joint moved along the camera's own line of sight
barely moves on screen in that facing and moves fully in a facing 90 degrees
off it. Three frames, because every frame of every direction is about 37,000
lines and would be rubber-stamped. **A missing golden is an error**, never an
auto-accept, and `MARROWFALL_UPDATE_GOLDENS=1` is the only thing that rewrites
one:

```sh
MARROWFALL_UPDATE_GOLDENS=1 cargo art run survivor --only bake
git diff art/goldens/                     # read what moved before committing it
```

With that variable set the rule reports `skipped` rather than a reading:
comparing a run against a file it just wrote is the run agreeing with itself.
CI asserts the variable is unset before it runs a test, and a unit test reads
the workflow to say so.

Three `atlas.*` rules run at the **pack** boundary, on the manifest and the
atlas it indexes. `atlas.manifest_schema` runs `sprites::parse`, the reader
the game loads the file with, so every invariant the game holds a manifest to
is that one rule and nothing here describes the format twice.
`atlas.frame_count` is one rect per direction per frame, and
`atlas.trim_boxes` is every rect and the anchor inside the atlas image and
inside its own cell: the numbers behind two of those invariants, plus the one
thing no format check can see.

## The contact sheet

Packing draws one picture of every animation in every direction, at final
sprite size, read from the packed atlases so what is reviewed is exactly what
ships. It is written twice: full resolution under `art/preview/<name>/`, which
is gitignored local scratch and what CI uploads as an artifact, and
downscaled to `project/assets/characters/<name>/sheet.png`, which is
committed under Git LFS beside the atlas it pictures. The art and its picture
are one diff, and a pull request approval is the sign-off.

`cargo art check --sheet` draws both from the atlases already committed, with
no bake and no Blender, which is what CI runs. The redraw is deterministic, so
CI then asserts the committed copy came back byte for byte: a thumbnail that no
longer matches the atlases is a sign-off on the wrong art.

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
lock, so a completed concept stage is not re-run without `--retry`,
`--from concept` or `--only concept`.

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

Every mesh rule measures **world space first, then welded**, and says so in
its finding. glTF splits one vertex at every UV seam, so a naive read of the
survivor counts 13,368 boundary edges on a mesh that has 171. The weld
distance is 1e-5 m, chosen from a merge histogram whose plateau runs from
1e-9 m to 1e-4 m. Every `[profile.mesh]` limit is calibrated on a real bare
mesh, `art/staging/survivor/spike/unset/bare.glb`, with the reading beside it
in `art/skeletons/humanoid.toml`.

Two rows are not what a rigged file would suggest. `mesh.self_intersect` is
read on the **pair**, because mirroring copies the crossings of the half it
keeps: 1094 arrive and 1153 survive the fixer. And `mesh.world_size` has a
band of its own rather than the rig's 5 percent, because `height_meters` is a
parameter of the **rigging** call: nothing before it scales the body, and a
bare mesh arrives about 1.90 m tall whatever the spec asks for.

## Renaming and conforming the rig

A bought rig is not a conformant one. Meshy names its bones its own way and
numbers its spine from the top, so its `Spine` is our `Spine2`; it points the
`Hips` bone at a hip socket, pitches `Head` off its own end, and places limb
joints 1 to 4 percent asymmetric even on a mesh the fixer symmetrized. Every
one of those was measured on the three rigs `spike-pose` bought.

So the download stage does not write `model.glb`. It writes
`art/staging/<name>/rigged.glb` and takes it the rest of the way:

```text
rigged.glb -> rig gates (record) -> rename -> conform -> conformed gates
           -> art/characters/<name>/model.glb
```

**A bought clip takes the same detour, for the same reason.** Meshy animates
the rig it sold, so the delivered file carries the vendor's bone names and the
vendor's rest pose, and an action's keys are read against a rest pose. A clip
kept as it was downloaded would put the vendor's rest frames back on a body
the conform has just moved. So it lands in `art/staging/downloads/` as a
source and reaches `art/animations/` the way a Mixamo clip does:

```text
<clip>.glb -> check_source.py -> retarget_animation.py --convention meshy
           -> clip gates -> art/animations/<clip>.glb
```

The fit is also what drops the stock character a clip arrives with: a provider
ships the whole mesh and its 2048-square texture with every motion, and these
files are committed.

**The rename** reads which convention the file is in from `[fingerprints]` in
`art/skeletons/<skeleton>.toml` and rewrites every joint name by role into the
canonical one. It is a JSON chunk edit: glTF addresses a joint by node index,
so a bone's name is one string in one place and the buffer chunk comes out
byte identical. A file already in the canonical names comes back byte for
byte.

**The conform** turns every joint whose own axis misses the child
`[profile.tails]` names onto it, by the smallest rotation about `old x new`,
and when `symmetry` is on it first averages each mirrored pair onto X = 0.
Every joint keeps its world position, every child's local transform is
re-expressed, and every inverse bind matrix is recomputed against the joint's
new world matrix. That last one is why the mesh does not move: glTF skins a
vertex through `world(joint) @ inverseBind(joint)`, so holding that product is
holding the vertex. Measured on the committed rig, the joints move
millimeters and the furthest rest vertex moves **1.57e-7 m**, which is the
`f32` a GLB stores.

**Two stage names over one rule set**, the way `mesh` sits beside `cleaned`.
`rig.<name>.1.json` is what the vendor shipped and it refuses nothing: a
bought rig fails the three name rules by construction, and the rename is what
closes them. `conformed.<name>.1.json` is the gate. `cargo art check` writes
the same pair from whatever is on disk.

One defect no rest-frame edit can close: `rig.humerus_angle` reads the
direction from the shoulder joint to the elbow, which is where the mesh's arm
is. The survivor's arms hang 59 degrees below horizontal against a prompt of
40, and moving the joints to reach it would put the bones outside the arms
they deform. That one is a generation defect, and the `pose_mode` spike below
is what measured the only lever on it.

### Which `pose_mode` to send

```sh
cargo art spike-pose survivor          # about 90 credits, then free
cargo art spike-pose survivor --rig    # and 5 more per mode
```

Meshy's `multi-image-to-3d` takes an optional `pose_mode`, and which value to
send is a measurement rather than a preference. The command reconstructs the
same four committed concept views three times, unset, `a-pose` and `t-pose`,
each into `art/staging/<name>/spike/<mode>/`, runs the fixer and every `mesh.*`
rule on both files, and draws a contact sheet of five views of the bare mesh:
front, back, left, right, and the arms alone from 45 degrees above the front,
which is where a forearm's top surface is. `--rig` then buys a skeleton for
each and reads every `rig.*` rule on it.

Free to re-run: a mode whose `bare.glb` or `rigged.glb` is already on disk is
measured again and bought again never. It asks before it bills, quoting only
what is left to buy, so a second run asks nothing. It is not a stage, writes
no lock, and touches no committed art; each mode files its reports under
`art/staging/reports/<set>.<name>-<mode>.1.json`.

It is the one command that measures a mode whose mesh gates failed instead of
stopping there, and the reason is narrow: the limits are what it is
calibrating. The `rig` stage still refuses, and `spike-pose` still exits
non-zero carrying the verdict.

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
what `art/characters/<name>/spec.lock` records:

| Stage | Cost | Runs in | Does |
|---|---|---|---|
| `concept` | paid | Rust | Turns the written description into four concept views, gated and retried up to three times. |
| `model` | paid | Rust | Concept art to a textured 3D mesh. |
| `rig` | paid | Rust | Adds a skeleton, then one animation clip per entry in `animations`. |
| `download` | free | Rust | Fetches the finished GLBs, renames and conforms the rig into `model.glb`, then fits every bought clip onto it. |
| `bake` | free | **Blender** | Renders every clip through 8 compass directions into loose PNG frames. |
| `pack` | free | Rust | Crops and packs those frames into one atlas per clip, plus the manifest Godot reads, the three `atlas.*` rules over both, and the contact sheet. |

The `.ron` spec describes the *character*; the `.lock` records *what has been
built*. So `pack` appears in the lock without appearing in the spec, its
settings live in the spec's `bake` block, which is one block of sprite settings
shared by the last two stages rather than one block per stage.

The pipeline is Rust. `tools/blender/src/` holds the only Python in the repo,
because `bpy` is Python-only and Blender is the one tool that cannot be driven
any other way: `bake_sprites.py`, `check_source.py`, `mesh_clean.py`,
`retarget_animation.py` and `mesh_sheet.py` run inside Blender, and
`cleanup.py`, `clip.py`, `findings.py`, `framing.py`, `skeleton.py`,
`source.py` and `transfer.py` are the `bpy`-free modules they import.
`strip_animation.py` is neither: nothing shells out to it any more, and the
retarget imports its one-triangle skin carrier.
`cargo art` shells out to `blender --background --python …` for those five,
and does everything else itself. Every published limit those scripts report
against is passed to them as `--limit RULE=NUMBER`, read off the same rule
list `--list-rules` prints, so no script holds a second copy of a number. It
finds `blender` on `PATH`, or at the path `MARROWFALL_BLENDER_BIN` names when
it is set.

## What the lock covers, and how to force a stage

Every stage's fingerprint covers the spec fields it reads and the content of
the files it opens, so a replaced input cannot report `cached`:

| Stage | Reads |
|---|---|
| `concept` | the description, and the pose its body plan injects |
| `model` | the remesh and texture settings, and all four concept views |
| `rig` | the height, the skeleton, `cleanup`, `symmetry`, the Meshy action ids, `bare.glb` and `clean.glb` |
| `download` | the same as `rig`, plus `rigged.glb` and `humanoid.toml`, which the rename and the conform read |
| `bake` | the sprite settings, `model.glb`, `humanoid.glb`, `humanoid.toml`, every animation GLB it plays, the Blender build and every script |
| `pack` | the name, `sprite_height`, the directions, and which clips loop |

The landmark goldens are in no row: they are what a bake is measured
**against** rather than an input it reads, so rewriting one never makes a
stale bake current. The contact sheet is the same, one step later.

The `download` row is narrower than what that stage opens: it fits every
bought clip through `retarget_animation.py`, and no Blender script is in its
fingerprint. `LOCAL_PIPELINE_VERSION` is what covers a script edit there, per
the rule below, and keeping Blender out of this row is what lets
`cargo art status` answer on a machine that has none.

`pack` is the exception: it reads hundreds of staging PNGs, and the skeleton
profile, and hashes none of them. That is safe because those PNGs have exactly
one author, and because no `atlas.*` limit is a profile number: all three
count defects, and the profile is loaded only because `Rule::measured` takes
one. Recording a bake clears every
stage after it, so a re-bake always re-packs, and `stages::bake` deletes the
`*.png` it is about to rewrite, so no frame of an older shape survives to be
packed. The profile is in the `bake` row above, so an edited limit invalidates
the bake and forces the re-pack anyway.

`model`, `download`, `bake` and `pack` also carry `LOCAL_PIPELINE_VERSION`,
because a fingerprint over inputs cannot say "the code that produced this was
fixed". `concept` and `rig` do not: a local fix must never re-spend on OpenAI
or on rigging. Bumping it does re-run the model stage, which costs Meshy
credits.

Content, never a date, so a fresh checkout is not a rebuild. A **committed**
input that is missing is an error rather than a hash of nothing:
`humanoid.glb`, `humanoid.toml` and the scripts have to be there. Everything
the pipeline **produces** reads the word `absent` until it exists, which is
why a character with no concept art yet can still be planned.

`bare.glb` and `clean.glb` are derived and gitignored, so on a machine that
does not hold them the `rig` record reads stale. Nothing spends on that: while
the committed `model.glb` is on disk a plan skips every stage up to
`download`.

Two commands can spend money, and both are typed by hand. `cargo art run`
bills only when it is forced: `--from concept`, `--from model` and `--from
rig`, the same three with `--only`, and `--retry`. And `cargo art spike-pose`
bills up to 105 credits, 90 of them without `--rig`. A paid stage the lock
still calls **current** asks before it bills. A paid stage the lock calls
**stale** does not ask, because forcing it was already the answer, so
`cargo art status <name>` never recommends one: it lists every
stale stage, says what each paid one would bill, and offers the earliest free
stage as the one command that rebuilds for nothing. Every paid stage runs
before every free one, so `--from` a free stage cannot reach a bill.

A crash while a paid task is in flight does not re-spend either. The id is
written to `art/staging/reports/<stage>.<item>.<attempt>.task` before the
first poll and removed once that task reports success, so the next run polls
the one already paid for instead of buying a second. It is removed on success
and not later, because `--retry` has to be able to buy a genuinely new one,
and on a task the provider gave up on, because that one reads the same way
forever.

The Blender build is an input too, and only the `bake` row reads it. On a
machine with no Blender that row reads `unknown` with the reason, the other
five still report, and `bake` drops out of the recommendation. A `run` whose
plan reaches the bake says the same thing before the first paid stage instead,
and `run --only concept` and `run --only model` need no Blender at all.

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
canonical rig. The download stage runs those same three on every clip Meshy
animated, so no clip reaches `art/animations/` unmeasured or unfitted.

Every export is requested **traveling**, so the root carries
the motion and the femur ratio can size the step to our own body, and at the
rate `source_fps` declares. `source_fps` is the clip's own rate and `fps` is
the sprite sampling rate: `idle` is sampled 8 times a second out of a clip
authored at 24.

Each fetch is recorded in `art/animations/library.lock`: what arrived, what
was kept, the rig and tooling that fitted it, and the **verdict** its gates
reached, which is every rule that reported, the worst severity, and the report
to open. The vendor FBX is not committed, so that record is the only evidence
an uncommitted input was ever measured. A clip with no verdict, with a failing
one, or fitted by a rig or a Blender that has since changed is fetched again,
and the step says which of those it was.

The first run opens Chrome at Mixamo and waits while you log in, because
exporting needs a session credential that lasts about a day. Nothing is stored:
the credential is read from the browser each time it is needed.
