# Design: Art Pipeline Foundations

## Context & Problem

The art pipeline turns a text description into packed sprite atlases and
ships broken art while reporting success. The facts table below carries the
measured damage. Checks were written to catch exactly this, and every one is
structurally unable to fail or unable to be right, so each patch landed
green. The problem is not one bad transform. Nothing in the pipeline can tell
right from wrong, so no fix can be trusted and no agent can work here
unsupervised. Every decision is settled in
`docs/design/2026_08_20_art_pipeline_decisions.md`, with evidence in the 74
reports under `docs/research/agent_reports/`. One item is a spike with a named
acceptance test.

This document runs about 1,355 lines against the template's stated 1,000. The
overrun is the 51-row test table that decision 11 makes the contract, the
18-row facts table that thirteen sections cite by number, and one question per
settled decision. Reaching 1,000 means deleting one of those three.

## In Scope

- One written skeleton spec, stored as data, plus a check that says which
  rules a rig breaks and by how much, and a rename to the standard names.
- An in-house retarget that transfers motion in world space against an
  absolute per role aim table, replacing the current local-space maths.
- A verifier that measures swing and twist separately, terminal bones
  included, against the original vendor file.
- Foot planting, floor snapping, and stride scaled by femur length, on
  traveling source clips.
- Two mandatory properties for every gate: a negative control that proves it
  can fail, and a calibration that proves it stays quiet on known-good art.
- Local mesh cleanup and symmetrize, opt-in per character, on for the
  survivor, running on the bare mesh before rigging.
- Gates at every stage boundary, plus automatic regeneration when a concept
  gate fails, and a lock that fingerprints real input files.
- Review artifacts committed beside the atlas, so pull request approval is
  the art sign-off.
- One end to end test that launches Godot and loads every atlas.

## Out of Scope

- **Uploading our art to Mixamo.** Their auto-rigger needs hand-placed
  markers.
- **Meshy's paid AI Auto-Repair.** It strips textures, and rigging refuses an
  untextured mesh, so it breaks the stage it exists to serve.
- **A hand-authored skeleton**, and **a strict T-pose re-bind.** Decisions 6
  and 2.
- **`concept.arm_angle`.** A numeric limb angle needs pose-estimation
  landmarks, not a silhouette, and it is unverified whether models trained on
  photos work on stylised art (`research_concept_and_model_stage.md:24`).
  `rig.humerus_angle` catches the same defect one stage later, for free.
- **Waivers and a severity override.** Decision 11 forbids them.
- **Color pixel goldens of the bake, and Godot frame goldens.** EEVEE output
  varies by GPU (`research_art_pipeline_qa_systems.md:70`).
- **A vision model as an image judge.** Classical image checks come first.
- **Idempotency keys.** Neither provider implements one and the SDK never
  sends the header (`research_concept_and_model_stage.md:66`).
- **A license provenance gate.** Adobe's Mixamo terms were unreadable to every
  research agent, so the rule has no source of truth
  (`research_unattended_art_pipelines.md:224`).
- **Deferred, not dropped:** the `illegal` field, source IK baking, joint
  limits, keyframe simplification and twist bones, each with its cost under P2
  and P3 in `mine_retarget_bvh_and_find_our_gaps.md`.
- **Changing the game side, the packer, the manifest format, the camera or
  the framing maths.** The audit found them sound, so sprite rates and
  playback speeds do not move.

## Terminology

- **Rest pose (bind pose):** where bones sit with no animation applied.
- **Aim table:** the absolute world direction we want each role's bone to
  point in, per skeleton, stored as data. An input, never a pose.
- **Reference pose:** the pose each rig reaches when aimed by the table with
  its own roll kept. Recorded per rig, used only to compute the offset.
- **Swing:** where a bone points, the two degrees that move its child.
- **Twist (roll):** rotation about a bone's own length. It moves no child
  joint, so a direction metric cannot see it.
- **Weld:** merge coincident vertices. glTF splits one vertex at every seam,
  so topology must be welded before it is counted.
- **Non-manifold edge:** an edge shared by three or more faces. A **boundary
  edge** (a hole) is shared by one. An **island** is a connected component.
- **Self-intersection:** a pair of triangles from the same mesh that cross
  without sharing a vertex or an edge. Counted as faces.
- **Mirror plane:** the left-right plane through the character, X equals 0 in
  our rigs. Mirror rules reflect across it.
- **Root motion:** the travel the hips carry across a clip.
- **Two bone analytic IK:** solving a knee or elbow in closed form so a foot
  reaches a target, with no iteration.
- **Depsgraph:** Blender's dependency graph. Re-evaluating it per bone per
  frame is what makes the current retarget 1,000 scene evaluations.
- **Gate:** one check that can stop the pipeline. It carries a rule id, a
  measured number, a limit, a comparison, a unit, a space, a severity.
- **Comparison codes:** `le` at most, `lt` strictly less than, `eq` exactly,
  `ge` at least.
- **Negative control:** a test that feeds a gate a broken input and asserts
  the gate fails. Without one, a passing gate proves nothing.
- **Known-answer test:** the expected output computed once by a human and
  committed as a **number**, never as a comparison against the code under
  test. The only external ground truth in the suite.
- **Metamorphic relation:** a rule about how outputs must change when inputs
  are transformed, used where nobody can write the expected output.
- **Calibration:** known-good art in, silence out.
- **Golden:** a committed expected result. Ours is text, not an image.
- **Contact sheet:** one PNG holding one rendered frame per direction, so a
  whole animation is judged in one look.
- **The `skipped` exemption:** a rule that a spec field switches off reports
  `severity: skipped` with a message saying which field. It needs no negative
  fixture, because it cannot stop a build. The only exemption to rule one.
  **`info` is a different thing and T2 separated the two:** every rule reports
  its measurement, and `info` means measured and not a defect, either inside
  its limit or under a rule with no limit worth failing, such as
  `mesh.quads`. A rule that goes quiet when it passes cannot be told from a
  rule that never ran. Both kinds live in one report, so a consumer reads the
  severity and never the prose. Four severities: `error`, `warning`, `info`,
  `skipped`.

## Key Decisions

Every decision rests on a measurement. Where a measurement was later found to
be wrong, both readings are shown, because how that happened is part of the
design.

| # | Measured fact | Where |
|---|---|---|
| 1 | Rig: elbows bent 24 deg at rest, arms 59 deg below horizontal against a spec of 40, 0.9 to 3.7 percent segment asymmetry, `Hips` +Y points out of the left hip at `(-0.980, -0.145, -0.136)`, `Head` pitched 30 deg, 2 of 24 bones fill no role, importer-invented tails 100x too long. **T2 re-measured every figure from the glTF node graph**: elbows 24.1 and 23.8, arms 59.1 and 59.3, asymmetry 0.95 to 3.68 percent, the `Hips` axis to three decimals, 2 of 24 roleless. Two are restated as rules rather than as a pose: `Hips` is 97.6 deg off the direction to its child, and `Head` is 26.0 deg off the direction to `head_end`, where its own node rotation is 27.6 deg. | `audit_the_current_art_pipeline.md:45`, `check/rig.rs` |
| 2 | `rotation_difference` is blind to twist, so the shipped clips carry **our own rest twist unchanged**. `sole_children` leaves 7 bones uncorrected: `Hips`, `Spine`, `Head`, both hands, both toes. Output error: wrists 52.9 and 66.5 deg **out of their own rest**, `spine_lower` 9.9 deg. | audit, `retarget_animation.py:264` |
| 3 | Legs and feet differ from Mixamo's by **171 to 175 deg of pure roll at rest**, and the current direction metric reads 5. A correct retarget preserves that difference. | audit line 74 |
| 4 | The shipped verifier prints `left_hand 0.0 deg` on a clip whose left wrist is 52.9 deg out of rest, because `LIMB_CHAIN` names each segment after its distal bone and no terminal bone is measured. | audit section 5 |
| 5 | `strip_root_motion` assumes `Hips` local Z is world Z. It is world minus Z tilted 8.9 deg, so a left strafe sinks 0.32 m and a right strafe rises 0.39 m, against a real bob of 0.043 m. It also pins the horizontal channels to their first-frame value. | `bake_sprites.py:549` |
| 6 | glTF stores key times in seconds. A 30 fps clip re-imported into a 24 fps scene lands at range 0.8 to 16.8. `library.ron`'s `fps` is the **sprite sampling rate** (idle 8, walk_back 20), not the source rate, and `framing.py:260` already rounds every sample frame to an integer. | `mine_retarget_bvh_and_find_our_gaps.md:440`, `library.rs:76` |
| 7 | `transform_apply(scale=True)` rescales rest geometry and leaves location keys byte identical, so their meaning changes 100x. Hips world head goes from 2.316 m to 231.599 m. | same report, section 3 |
| 8 | Mesh topology, one file, three states: raw glTF 13,368 boundary edges, welded 171, welded and cleaned 27. Welded non-manifold 8, self-intersections 16, 27,508 vertices. Before welding: 34,490 vertices and 54,864 tris. Islands: 7 inside `char1`, plus the stray `Icosphere` as a separate object. Every figure is `model.glb`. | `research_meshy_mesh_repair_options.md:22`, `research_concept_and_model_stage.md:37` |
| 9 | Mesh mirror asymmetry on `model.glb`: mean 0.50, p99 1.76, max 3.02 percent of width, by an independent kd-tree pass. `symmetrize` ran in 55 ms and drove it to 0.0. `symmetry_mode` is deprecated. | `research_concept_and_model_stage.md:72` |
| 10 | `art/characters/survivor/model.glb` is already the rigged, skinned file. Cleaning geometry there desyncs skin weights: a trial changed vertex count by minus 23. **`bare.glb` has never been downloaded or measured, so no `mesh.*` limit exists yet.** | `research_meshy_mesh_repair_options.md:59` |
| 11 | `POST /print/analyze` costs **0 credits**. `POST /print/repair` is 10 credits and **strips textures**, and rigging refuses an untextured mesh. Meshy credits run about 0.013 USD each, from 20 to 30 credits at 0.25 to 0.40 USD. | Meshy API reference, `research_concept_and_model_stage.md:23` |
| 12 | Rigging accepts `model_url` as a **URL or data URI** of a textured `.glb`, costs 5 credits, and `input_task_id` wins if both are sent. With `model_url` the character must face **+Z in glTF Y-up**. No body size limit is stated. The face limit of 300,000 is stated for `input_task_id`. | `reference.md:643,644,651` |
| 13 | Meshy's extension returns `FINISHED` from `delete_small_pieces` having deleted nothing, because it measures piece volume in **local** space while its checker measures in **world** space. | `research_meshy_mesh_repair_options.md:54` |
| 14 | `lock.rs` fingerprints no art file, and `LOCAL_PIPELINE_VERSION` covers only `Bake` and `Pack`. The rig can be replaced and every stage reports cached. `Stage::Concept` bills OpenAI, so `costs_credits()` is already true for it. | `lock.rs:72,237,243` |
| 15 | `--python-exit-code` catches only top-level exceptions. Raised from a `bpy.app.handlers` callback, `atexit`, `unregister` or a thread, Blender **exits 0**, so the success sentinel is the real gate. | `proof_python_exit_code_coverage.md` |
| 16 | Blender ships no official Linux arm64 build and our CI runner is `ubuntu-24.04-arm`, so nothing in CI can call `bpy`. The npm `gltf-validator` is Dart compiled to JS, so it is architecture independent. | blender.org, `pr.yml:18`, Khronos npm README |
| 17 | The hunched posture is in the Mixamo source: `strafe_left` is authored with the head 34 to 37 deg forward. Our output copies it faithfully. | audit |
| 18 | `stages::concept` reuses images on disk unless `force` is true, and `stages::retarget` is called from exactly one place, the Mixamo fetch path. | `stages.rs:35`, `cli.rs:459` |
| 19 | A swing-twist split must project the quaternion's **vector part**, not `Quaternion.axis`, which is normalized and drops `sin(angle / 2)`. On a pure 10 deg twist the wrong form reads twist 90.22 and swing 80.22 against a true 10.00 and 0.00, and it is worst **near identity**, where a pure swing hides it. | `proof_swing_twist_vector_part.md` |

### How do we know a gate can fail, and that it measures the right thing?

Checks here have failed in three ways.

- **Five can only pass.** The shipped verifier never measures terminal bones
  (fact 4). The prototype's "0.000000 degrees" restates the formula it just
  applied. Twelve tests in `test_framing.py` assert a function against
  itself. The prototype's reference pose skips the same seven bones as the
  code it replaces (fact 2).
- **One was confidently wrong.** The mesh audit read 13,368 boundary edges
  against a true 171, and a second tool agreed to the integer because it
  shared the representation (fact 8).
- **One reports success having done nothing** (fact 13).

The last two are the dangerous shapes, because they produce confident output
that reads like evidence. Independent implementations alone do not help:
Knight and Leveson found 27 independently written programs failing together
far more often than independence predicts
(`research_art_pipeline_qa_systems.md:259`).

#### ✅ Option 1: Four rules, applied to every gate

1. **No gate merges without a negative control.** Per decision 11 the
   negative fixture is the **current committed art** wherever real broken art
   exists. It is better than synthetic: it is the failure we shipped.
2. **No gate merges without a calibration**, and every rule names the
   representation and the space it measures in.
3. **A fixer is never trusted by its return code.** Every repair is followed
   by re-running the gate that asked for it, with a strict comparison.
4. **All CI-side measurement is Rust, and Python measures only what needs
   `bpy`.** One reason: fact 16, and rule one is worthless if a negative
   control cannot run in CI.

```python
# mesh.holes: WELDED geometry, coincident verts joined at 1e-5, WORLD space.
# Negative fixture: bare.glb with the weld step disabled, over 13,000 edges.
def test_the_mesh_gate_rejects_the_art_we_shipped():
    assert mesh_check(BARE_GLB, weld=False).worst("mesh.holes").is_error()
```

Rule four decided concretely: rig rules, mesh topology, glTF world transforms
and the object-transform invariant live in Rust under `check/`. Blender emits
Findings only for the fixer, the retarget and the bake, and the Rust side
parses those. `mesh_check.py` does not exist.

**Pros:** each rule kills one named failure shape, and rule four turns every
mesh and rig negative control into a required CI check rather than a local
courtesy, with one Finding implementation on the enforcing side.

**Cons:** 30 to 50 percent more test code per gate, and measuring topology in
Rust costs T3 half a day more than `bmesh` would have.

**Rationale:** Accepted. Self-critique collapses while sound external checking
helps, and an imperfect gate caps accuracy no matter how many retries you
spend (`research_art_pipeline_qa_systems.md:320`). Fact 19 is the case in
point: a one-line error inside the design's own "whole design" step survived a
known-answer test that compared against a large expected value.

#### ❌ Option 2: Round trips, higher coverage, and a second implementation

```python
assert retarget(retarget(clip, A, B), B, A) == clip   # the trap
```

**Pros:** cheap, and it reads like thorough testing.

**Cons:** all three are in place today and all three failed. A round trip
passes when a transform is applied wrongly but consistently, coverage proves
only that the code ran, and the mesh numbers were cross-checked against a
second tool that agreed and was wrong.

**Rationale:** Rejected. This is the status quo and the status quo is the bug.

### What does "the clip is correct" mean, as a number?

One combined angle cannot work. Our legs sit 171 to 175 degrees of pure roll
from Mixamo's at rest (fact 3), and a correct retarget **preserves** that. A
combined metric reads about 174 on perfect output, the limit gets widened to
175, and the 52.9 degree wrist becomes invisible. That is the original bug
with a new name.

#### ✅ Option 1: Two rules. Swing absolute, twist relative to each rest

```
clip.swing(b,t) = angle( +Y_world(out,b,t), +Y_world(src,b,t) )          # ~0
clip.twist(b,t) = twist(out,b,t) - twist(out,b,rest)
                - ( twist(src,b,t) - twist(src,b,rest) )                 # ~0
    where twist(r,b,t) = rotation of bone b about its OWN +Y axis, from a
    swing-twist split of its world rotation at frame t, per fact 19
```

- **`clip.swing`** is absolute against the vendor file. It reads about 0 on
  the legs, because both rigs' bones point the same way, and it is the rule
  that sees fact 4's unmeasured wrist.
- **`clip.twist`** compares each rig's twist against **its own rest twist**,
  read from the vendor FBX for the source and from `humanoid.glb` for ours.
  Still external truth, and it catches a re-rolled thigh while ignoring the
  174 degrees of convention difference.
- **Every mapped bone, terminals included**, which is exactly what the old
  code left uncorrected and the old metric cannot see.
- **Frames align by seconds from clip start**, not by index, because
  requirement 9 removes a key so the ranges can differ by one.
- **Both limits are set by T6** from the committed CMU cross-rig clip. No
  number is published in advance, because the only figures we have were
  measured with the old combined metric on a same-rig pair and are not swings.

**Pros:** sees the wrist, the 9.9 degree spine and a re-rolled thigh without
failing on the convention difference. Matches published practice, which
compares global joint state normalized by character height
(`research_art_pipeline_qa_systems.md:135`). A failure names the bone.

**Cons:** two rules and two limits, and a cross-rig calibration needs a third
rig, so T6 commits one small CMU BVH clip, free for any use.

**Rationale:** Accepted. One limit for two quantities was the bug.

#### ❌ Option 2: One combined orientation angle

```
error = angle_between(world_out(b,t), world_src(b,t))   # 174 on the legs
```

**Pros:** one rule, one number, one limit.

**Cons:** it must reject every correct clip, per fact 3, and widening the
limit past 174 blinds it to the defect it exists to catch.

**Rationale:** Rejected.

### Which retarget implementation?

Settled by decision 7. Every candidate was installed, run headless on Blender
5.2 and measured on the same clip with the same absolute metric.

| Tool | Worst absolute error on our rig pair | Why not |
|---|---|---|
| **Ours** | **about 0.1 deg** | one latent coverage hole, closed by requirement 2 |
| Rokoko beta | 9.5 to 43.9 deg | no world-space rest compensation |
| retarget_bvh, stock | 9.5 to 33.3 deg | limbs-only aim table, and "Bend Positive" costs 33 deg of elbow |
| Rokoko stable | did not run | `Action.fcurves` removed in Blender 5.0 |
| Expy-Kit | did not run | no license file, so all rights reserved |
| Auto-Rig Pro | not testable | paid, no scripting API |
| Mwni | segfault | calls `popup_menu`, which needs a window |

#### ✅ Option 1: Build it in house

The ten requirements from the decisions file are the specification.

| # | Requirement | Proof |
|---|---|---|
| 1 | Never apply object transforms to a rig that owns an action. Read `matrix_world @ pose.matrix` | fact 7 |
| 2 | The aim table is absolute, per role, and covers every role, torso included | adding 5 torso rows to retarget_bvh took neck error from 14.3 to 0.00 |
| 3 | Bake scene fps equals the source clip's own fps, and every key time is asserted integral | fact 6 |
| 4 | Scale root travel by femur length, then snap the lowest foot frame to the floor | retarget_bvh floats the toe 6 cm and travels 11.8 percent short |
| 5 | Compare absolute angles against the source clip, never against the formula, never frame to frame | retarget_bvh transfers motion range to 0.5 deg while sitting 14 deg off |
| 6 | Foot planting: detect contact frames, lock foot XY, two bone analytic IK | contact frames exist, 0.6 mm step |
| 7 | Errors are typed values with a code and the offending bone | retarget_bvh returns FINISHED on failure |
| 8 | Local matrices computed algebraically, no depsgraph update per bone per frame | 1,000 scene evaluations today |
| 9 | Output keys are LINEAR with CONSTANT extrapolation, and no key at the reference frame | retarget_bvh leaves a T pose at frame 0 |
| 10 | The bone map is read from `humanoid.toml` only, with `parents`, `optional`, `fingerprint` added | the prototype hardcodes a second copy |

**How the reference pose is applied.** This step is the whole design, and
getting it wrong makes every offset identity or, per fact 19, quietly wrong on
every bone.

1. `aim_table[role]` gives a desired **world direction** for the bone. A bone
   with no role is skipped, not indexed, because 2 of 24 fill none (fact 1).
2. Pose each rig parents-first so the bone points there, **discarding twist**
   by a quaternion swing-twist split about the bone's own +Y, projecting the
   quaternion's vector part per fact 19. Not a `YZX` euler with
   `euler.y = 0`, which retarget_bvh uses
   (`mine_retarget_bvh_and_find_our_gaps.md:100`) and which is gimbal-locked
   at a Z component of 90 degrees. Our `Hips` lands near exactly that,
   because its +Y points out of the left hip (fact 1).
3. Record the **resulting** world matrices as two separate dicts,
   `ref_world_ours` and `ref_world_src`. Each keeps its own rig's roll, which
   is why the offset is a real correction rather than a re-roll.
4. `offsets()` takes only those two dicts. It never sees `aim_table`.

**Three assertions catch this whole class**, and each states a number:
`Offset(LeftUpLeg)` is a pure twist of about 174 degrees and never identity, a
pure 10 degree twist splits to 10.00 and 0.00, and a pure 30 degree swing
splits to 0.00 and 30.00.

Two things we do not copy. **retarget_bvh's "Bend Positive"** clamps a raw
quaternion component assuming local X is the hinge, which cost 33 degrees of
elbow. **Rokoko's 25 frame chunking** worked around a quadratic bake before
Blender 3.5, and on 5.2 the bake is linear and chunking is 1.07x slower.

Borrowed, by technique number from `mine_constraint_family_retargeters.md`:

| Techniques | What we take | Serves |
|---|---|---|
| 7 | compose both object matrices instead of applying them | req 1 |
| 32, 37 | scale by leg length, compensate every location key in the same operation | req 4 |
| 39 | a floor clamp on the root as a minimum on world Z | req 4 |
| 46, 48, 50 | stride matched to plant frames, a per role foot flag, a ramp in and out of each lock | req 6 |
| 64, 65, 66, 67 | collect every frame before writing any, `convert_space` POSE to LOCAL, bulk `foreach_set`, quaternion continuity | the bake |
| 17, 29 | refuse a duplicate target bone, resolve every data path first | req 7 |

**Pros:** verified on our own files at about 0.1 degrees on every joint. Twist
never enters the swing maths, so the rig's arbitrary rolls stop breaking
motion. No new dependency in a headless build, `humanoid.toml` stays the one
place bones are mapped, and the maths is a pure function, which is what makes
rule four's CI tests possible.

**Cons:** we own the maths, mitigated by the known-answer, negative-control
and metamorphic tests we need anyway.

**Cost:** T4 to T9 total 12.5 engineer days, foot planting included.

**Rationale:** Accepted. Every proven option is wrong by 9 degrees or more, or
does not run headless, or cannot be redistributed.

#### ❌ Option 2: retarget_bvh or Rokoko

```python
bpy.ops.mcp.retarget_selected_to_active(startFrame=1, endFrame=30)
```

**Pros:** mature, and patched retarget_bvh reaches 0.00 on elbows and neck.

**Cons:** the patches are a fork and the map moves into a second JSON copy.
Rokoko is rejected on accuracy, and it also flattens motion, disables TLS
checking process-wide, grows install-directory state every run, and has
nothing to pin. Full numbers in the decisions file.

**Rationale:** Rejected on measurement.

### What does the canonical skeleton spec say?

`humanoid.toml` maps 22 roles to bone names and says nothing about geometry,
so nothing has ever measured the rig. The names are also wrong in the one way
that matters: our `Spine` is the top and Mixamo's is the bottom.

#### ✅ Option 1: Structure rules plus a rename, bind pose left free

```toml
[profile]                             # every published limit lives here
bones = [ "Hips", "Spine", "Spine1", "Spine2", "Neck", "Head", ... ]
single_root = "Hips"                  # ancestor of every bone
meshes = [ "char1" ]                  # no Icosphere, no .001 names
up_axis = "z"                         # in Blender, after glTF Y-up import
facing_axis_gltf = "+z"               # in glTF Y-up, the space Meshy's 422 uses
child_axis = "y"                      # +Y points from a joint to its child
child_axis_tolerance_degrees = 2.0
height_tolerance_percent = 5.0        # against spec.subject.height_meters
mirror_tolerance_percent = 1.0        # left and right segment lengths
mirror_tolerance_degrees = 1.0        # left and right segment directions
humerus_below_horizontal = { target = 40, tolerance = 15 }
max_bind_deviation_degrees = 75       # catches "lying down", not "A-pose"

[profile.parents]                     # where each bone hangs
Spine = "Hips"

[profile.tails]                       # the one child `child_axis` measures
Hips = "Spine"

[aim_table]                           # world direction per role, degrees XYZ
hips = [ 90, 0, 0 ]
left_arm = [ 0, 0, -90 ]
```

**Two hierarchy tables, for two different questions.** `parents` says where a
bone hangs. `tails` says which child its own axis must point at, and a branch
bone needs it: `Hips` has three children and only `Spine` continues the body,
so the mean of the three points down and would reject a correct rig. Godot's
`SkeletonProfile` carries a tail beside every parent for the same reason. T2
adds it, and validates that every `tails` row is also a `parents` row, so the
two cannot drift.

Axis rules borrowed verbatim from Godot's `SkeletonProfileHumanoid`, the only
published machine-checkable rest contract
(`research_humanoid_rig_standards.md:52`). The two axis fields are asserted in
different spaces, which their names carry, and `rig.facing` ties the rig to
the bake camera's forward of minus Y.

**An axis is a choice of six, not a tolerance.** `rig.facing` and
`rig.up_axis` name the closest of the six signed axes and compare that, so
both accept anything inside 45 degrees of the declared one, by design: a lean
is not an axis error. A yaw between the two is `rig.mirror_direction`'s to
catch, because reflecting one side turns a yaw of 30 degrees into 60 degrees
between the sides, measured against a limit of 1.0.

| Field | What it does | Needed for |
|---|---|---|
| `parents` | retargeting hierarchy, decoupled from real parenting | a 3 spine source driving a 4 spine target |
| `optional` | a role allowed to be missing | a source with no shoulder or no toes |
| `fingerprint` | a bone that must exist for a convention to match | the **next** skeleton: after the rename our two tables are identical |

**One height rule, not three.** `rig.world_height` measures the rig against
`spec.subject.height_meters` with a 5 percent tolerance, so 1.6652 against 1.7
is 2.05 percent and passes. The old `world_height_meters` band is gone.

**Pros:** structure becomes rule-based while the bind pose stays an art choice
the retarget handles, so the expensive re-skin is not forced. Data, not code,
so a quadruped later is a second table read by one checker. The `bones`
allowlist names `head_end` and `headfront`, `meshes` catches the stray
`Icosphere`, and `rig.humerus_angle` closes the audit's defect 3, which
nothing has ever checked.

**Cons:** a wide bind-pose band accepts an odd rest pose, deliberately,
because the narrow check that matters is the reference pose measured per clip.
The rename invalidates the two committed Meshy clips, so it lands in T5, the
first task with the new retarget available.

**Rationale:** Accepted. There is no `[profile.severity]`: decision 11 means
nothing needs it, and the two spec flags below cover the one real case.

#### ❌ Option 2: Strict T-pose re-bind, or a hand-authored skeleton

```python
bpy.ops.pose.armature_apply()   # "Actions on this armature will be destroyed"
```

**Pros:** guaranteed conformant, and it matches every published standard.

**Cons:** the re-pose forces a re-bind of skin weights on 27,508 vertices for
a benefit the reference pose already delivers, and hand-authoring makes that
hand work recurring per character at 3 to 5 days each.

**Rationale:** Both rejected, decisions 2 and 6. The rename is kept.

### What do we do about the survivor's mesh?

| Defect class | Status after the transfer and profile decisions |
|---|---|
| Roll, elbow bend, arm angle, the sideways `Hips` axis, the 100x tails | Irrelevant. No code reads a bone axis or a tail as a world axis. |
| Topology debris: holes, non-manifold edges, tiny islands, the stray `Icosphere` | **Real.** Fused geometry makes weight painting unreliable, because binding cannot tell which vertices belong to which bone. |
| Asymmetry, up to 3.02 percent of width | **Real.** It is geometry, it renders, and auto-riggers assume bilateral symmetry. |

Both live mechanisms are Meshy's own
(`research_concept_and_model_stage.md:14`).

#### ✅ Option 1: Local cleanup and symmetrize, opt-in, before rigging

```ron
subject: Subject(
    kind: Humanoid,
    cleanup: true,      # weld, drop debris, fill holes
    symmetry: true,     # symmetrize, and enforce the mirror rules
)
```

**The ordering is not optional.** `model.glb` is already rigged and skinned
(fact 10), so cleaning there desyncs weights. Geometry work sits between the
`model` stage and `rig()`, on the bare mesh.

```
model stage -> download bare.glb  (new: the model stage fetches its own GLB)
            -> mesh gates on bare.glb, world space, then welded
            -> blender fixer -> clean.glb -> the same gates again
            -> POST /rigging { model_url: "data:model/gltf-binary;base64,..." }
            -> rigged.glb -> rig gates -> rename -> characters/<char>/model.glb
```

About 60 lines of plain `bmesh`. No addon, no credits, 55 ms. On `model.glb`
it took islands 8 to 1, holes 171 to 27, non-manifold 8 to 13, mirror error
3.02 to 0.00, and kept the texture and UVs.

**Those numbers do not become limits.** They were measured on `model.glb`, the
rigged file, while the gate and the fixer run on `bare.glb`, which has never
been downloaded (fact 10). T3 measures `bare.glb` and T10 measures the first
real `clean.glb`, and each writes its limits into `[profile]`. Until then every
criterion is relational.

**The two flags are independent.** `cleanup` alone decides whether the Blender
fixer runs at all, and therefore which limit set applies, pre-cleanup or
post-cleanup. `symmetry` alone decides whether symmetrize is part of that
fixer and whether the three mirror rules are errors or `info`. Flipping either
is a reviewed diff in `spec.ron`, and the negative control for each silenced
rule still runs in CI, so the proof that the rule works never leaves with it.

**Symmetry is per character, not global, and the reason is not convenience.**
Monsters can be asymmetric on purpose. A one-armed thing with a tail must not
fail a rule written for a human, and symmetrize must never run on it.

**Pros:** free in credits, deterministic, keeps the texture, and attacks the
two defects Meshy names as causes of bad auto-rigging before rigging credits
are spent. It likely fixes the skeleton's segment asymmetry too, because the
auto-rigger places joints from the mesh.

**Cons:** symmetrize deletes one half and mirrors the other without judging
which was right, **and it mirrors the UVs**, so an asymmetric texture detail
such as a strap or a scar is duplicated and flipped. The model contact sheet
is reviewed for both. Vertex count moves, so weights are recomputed, which is
why the re-rig is in the same step. Whether Meshy returns a symmetric skeleton
from a symmetric mesh is the bet, and it costs 5 credits to settle.

**Rationale:** Accepted, on for the survivor.

#### ❌ Option 2: Meshy's paid AI Auto-Repair

```jsonc
{ "model_url": "https://..." }   // POST /openapi/v1/print/repair, 10 credits
```

**Pros:** automatable, and it names holes and watertightness in its fix list.

**Cons:** it strips textures and rigging then refuses the mesh (fact 11), and
it makes no claim to fix self-intersections or merge islands.

**Rationale:** Rejected, decision 5. It fails the "no worsening" test twice.

#### ⏸ Spike, not a decision: which `pose_mode` to send

`image_to_3d_body` sends 8 fields and none is about pose. Empty lets Meshy
freestyle, which is how we got arms 59 degrees below horizontal against a
prompt of 40. Our concept views are A-pose, so `t-pose` makes Meshy invent the
tops of the forearms, while Meshy's own docs say `t-pose` rigs best. Those
conflict, so this is a measurement.

**Step 0, free.** `pose_mode` is documented for Multi-Image to 3D only by
inheritance: `reference.md:413` says the optional parameters are "Same as
Image to 3D" and `reference.md:382` lists it there. Send one request with the
field set and a deliberately invalid `image_urls`, and read the 400. An
unknown-parameter rejection costs no credits.

**Named acceptance test, `spike_pose_mode`.** Regenerate the model stage three
times from the same four committed concept views: unset, `"a-pose"`,
`"t-pose"`. Run every `mesh.*` and `rig.*` gate on each. `"a-pose"` is adopted
only if all four hold.

1. Humerus within 15 degrees of the 40 the prompt asks for.
2. Elbow bend at rest under 5 degrees.
3. Welded `mesh.holes` on `bare.glb` no worse than the T3 calibration.
4. A human confirms on the model contact sheet that no forearm surface was
   invented.

If `"a-pose"` fails and `"t-pose"` passes 1 to 3 but fails 4, the field stays
unset and the measurement is recorded. Cost 0.5 days plus about 90 credits at
0.013 USD each, so about 1.20 USD.

### Where do the gates run, and what happens when one fails?

#### ✅ Option 1: Stage boundaries own the gates. Concept failures retry

| Placement | What runs there |
|---|---|
| **Stage boundary in `cargo art`** | every gate, on the asset in hand, before the next stage and before more money is spent |
| **CI (`pr.yml`)** | every negative control and calibration, all in Rust, with no Blender. Plus `gltf-validator` and the goldens |
| **Pre-commit** | the same checks as a convenience mirror. Never a gate's only home, because hooks are editable from the branch and Claude Code overrides a blocking Stop hook after 8 consecutive blocks (`research_art_pipeline_qa_systems.md:314`) |

**The retry loop, per decision 10.** It lives in `cli.rs::run` and wraps the
concept stage only.

```
attempt = 1
loop:
    concept(&spec, &paths, /* force */ true)   # force: fact 18, or it reuses
    report = check(Stage::Concept, attempt)    # every concept.* rule
    if no error: break
    if attempt == 3: bail with all three reports, images kept on disk
    attempt += 1
```

- **Three generations in total**, one plus two regenerations, at four OpenAI
  images each. About 0.80 USD per attempt and 2.40 USD for all three.
- **`force` is mandatory.** Without it `stages::concept` reuses the images
  that just failed (fact 18), and the loop would spend nothing and change
  nothing while reporting a retry.
- It always regenerates **all four views**, because a new front view
  invalidates the three derived ones.
- **The wrapped stage is not free.** `Stage::Concept` bills OpenAI (fact 14),
  so `ConfirmSpend` is asked once, quoting 2.40 USD for up to three attempts.
  The invariant is that the loop **never wraps a Meshy stage**: a failing
  `mesh.*` or `rig.*` gate stops the run and reports.
- Each attempt writes `art/staging/reports/concept.<attempt>.json` and every
  Finding carries an `attempt` field, so all three attempts survive.
- The `concept.*` limits are calibrated in the same task that adds the loop,
  so no uncalibrated threshold can ever spend money.

**Two mesh gates, because they cost nothing and see different things.**
`print/analyze` is free, needs no Blender and runs on the bare mesh before
rigging. Its `non_manifold_edges` counts boundary and true non-manifold edges
together, which is why it reads 179 on `model.glb` where we read 171 plus 8,
and that goes in `measured_on`. Our own Rust pass measures the rest on welded,
world-space geometry. **Rule three has one stated exemption:** `print/analyze`
needs a URL and `clean.glb` is local, so the remote rule is not re-run after
the fixer. Ours are.

**Pros:** each failure is caught where it is cheapest, the cheap failure fixes
itself, and CI keeps a gate an agent cannot satisfy by editing a hook.

**Cons:** a doomed concept costs 2.40 USD before stopping, and some checks
exist in two places, which is the same function called twice.

**Rationale:** Accepted.

#### ❌ Option 2: One `cargo art check` command, run by hand

```bash
cargo art check && echo "looks fine to me"
```

**Pros:** the simplest possible thing.

**Cons:** self-declared, and the whole problem is that "the agent said it was
fine" was the acceptance criterion.

**Rationale:** Rejected. The command stays as a mirror, not as the gate.

### How does a human sign off on art?

#### ✅ Option 1: Pull request approval is the sign-off

Per decision 13. The contact sheet is committed under Git LFS **beside its
atlas**, in `project/assets/characters/<char>/`, so the art and its picture
are one diff. The pull request cannot merge without human approval, and an
author cannot approve their own
(`research_art_pipeline_qa_systems.md:295`).

There is no `cargo art review`, no `review.accepted` rule, no lock accept
field and no `Stage::Review`. There is also no `art/review/` directory:
`/art/preview/` stays gitignored local scratch.

**Clip audition, with an artifact.** At fetch time, before any retarget, the
source clip's own posture is measured: head pitch, spine lean, arm swing at a
few frames. Those numbers go into `art/staging/reports/fetch.1.json` as
`source.posture` findings **and** into the `verdict` field on `Fetched`, so a
hunched purchase is on record before the pipeline spends anything on it (fact
17). Nothing prints and vanishes.

**Landmark golden.** Project world joint positions through the bake camera and
commit 2D pixel coordinates as text. **Three sampled frames, two directions,
per clip**, not every frame of all sixteen: the full form is about 37,000
lines and would be rubber-stamped, and the camera is framed once across all
animations, so adding a clip rewrites every file.

```
art/goldens/survivor/strafe_left_e.txt
frame  bone          x     y
0      Hips        256   301
0      LeftHand    198   288
```

A missing golden is an error, never an auto-accept. Regenerating one needs
`MARROWFALL_UPDATE_GOLDENS=1`, which CI asserts is unset.

**Where a verdict is stored, and where it is not.** The retarget runs in
exactly one place, the Mixamo fetch path (fact 18), and the vendor FBX is not
committed. So the `verdict` field on `Fetched` in `art/animations/library.lock`
is the only record that an uncommitted input passed its gates. Committed
inputs need no stored verdict: `idle.glb`, `run.glb`, `humanoid.glb` and the
atlases are all in the repository, so CI re-derives their verdict on every
pull request. That is the whole rule. `fetch` and `retarget` are therefore not
new `Stage` variants, and the character lock keeps its six.

**Pros:** costs seconds of attention, only on real changes, and uses a gate
GitHub already enforces. The golden is the deterministic half and the sheet is
the judgement half. No new command.

**Cons:** committed sheets add repository weight, bounded by LFS.

**Rationale:** Accepted.

#### ❌ Option 2: A recorded accept in the lock, or a color pixel golden

```ron
Bake(accepted_by: "berkay", sheet: "a1b2c3d4")   // dropped
```

**Pros:** an accept survives a force-push, and a color golden catches shading.

**Cons:** the accept duplicates what a pull request approval already records
and the lock field is the abuse vector, and a published graphics diff still
flagged about 42 percent false positives
(`research_art_pipeline_qa_systems.md:80`).

**Rationale:** Both rejected, decision 13.

## Architecture Overview

Four changes in shape. Gates move to stage boundaries. All CI-side measurement
moves to Rust, so negative controls run on the arm64 runner. The retarget
maths moves into `transfer.py`, which never imports `bpy`. The `model` stage
gains a download and a local cleanup, so rigging is fed a cleaned mesh.

```text
spec.ron
  ▼ concept  ─▶ [concept.*  background, one figure, arm gaps, mirror, cross_view]
               fail ─▶ regenerate with force, 3 attempts total, 2.40 USD
  ▼ model    ─▶ download bare.glb
               [mesh.printability  print/analyze, 0 credits, no Blender]
               [mesh.*  WORLD space then WELDED: holes, non_manifold, islands,
                        self_intersect, mirror, world_size, facing, budget, uv]
               blender fixer (opt-in)  ─▶ clean.glb
               [mesh.cleanup_effective  lt]  [mesh.non_manifold_post  le]
  ▼ rig      ─▶ POST /rigging { model_url: data URI }   (5 credits)
               [rig.*  bone_set, parents, single_root, child_axis, mirror,
                       world_height, humerus_angle, facing, object_transform,
                       bind_deviation, up_axis, names_standard]  ─▶ rename
               [gltf.validator]
  ▼ fetch    ─▶ [source.*  posture, fps_declared, traveling]   (library.lock)
  ▼ retarget ─▶ [clip.fps_grid] [clip.object_transform] [clip.interpolation]
               [clip.swing  absolute vs the vendor file, every mapped bone]
               [clip.twist  change from each rig's own rest twist]
               [clip.foot_contact.*] [clip.floor_snap] [clip.loop]
  ▼ bake     ─▶ [clip.root_travel  on the stripped copy, max over frames]
               [bake.*  frame_count, non_empty, in_frame, pivot, forearm_roll,
                        sampled_frames_are_keys, landmark_golden]
  ▼ pack     ─▶ [atlas.*  frame_count, trim_boxes, manifest_schema]
  ▼ review   ─▶ contact sheet committed beside the atlas ─▶ PR approval

  transfer.py, plant.py (no bpy) ──▶ pytest in CI
  check/ (Rust, glTF) ──▶ cargo nextest in CI: every mesh and rig negative
```

## Third Party Dependencies

| Capability | Chosen | Alternatives | Why |
|---|---|---|---|
| Retarget solver | **None. Our own.** | retarget_bvh, Rokoko, Expy-Kit, Auto-Rig Pro, Mwni | all measured headless. Ours 0.1 deg, next best 33.3 |
| Mesh check before rigging | **Meshy `print/analyze`** | download and run Blender first | 0 credits, no Blender, on the bare mesh where failure is cheapest |
| Mesh cleanup | **Our own `bmesh`, ~60 lines** | `print/repair` (10 credits), Meshy's extension | repair strips textures, and the extension deletes nothing while returning FINISHED (fact 13) |
| Mesh and rig measurement | **Rust, `gltf` crate** | Blender `bmesh`, `bpy`-free Python over raw JSON | rule four plus fact 16. `gltf` is the standard reader and gives the node graph the rig rules need |
| Self-intersection counting | **`parry3d`, its `Bvh`** | hand-rolled BVH, `pymeshlab` | dimforge, maintained, ships the broad-phase `Bvh` plus triangle queries. `Qbvh` was removed and replaced by `Bvh`. This is the one measurement `bmesh` never did (`research_concept_and_model_stage.md:40`) |
| glTF structural validation | **npm `gltf-validator` under Bun** | Homebrew, a precompiled binary, a Rust wrapper | there is no Homebrew formula and the GitHub binaries are x64. The npm package is Dart compiled to JS, so it is architecture independent (fact 16), and `validateBytes()` returns a report with severities |
| Concept image checks | **`opencv-python-headless`** | Pillow plus numpy, rembg | the arm-gap check needs contour hierarchy (`findContours` with `RETR_CCOMP`) |
| Blender in CI | **Not used** | container, an x86_64 runner | fact 16 |
| Art sign-off | **Pull request approval** | Chromatic, Skia Gold, a lock field | decision 13. GitHub already forbids self-approval |

## Structure

```text
art/
  skeletons/humanoid.toml   # roles + parents / optional / fingerprint
                            #       + NEW [profile]   every published limit
                            #       + NEW [aim_table] world aim per role
  animations/library.ron    # + NEW source_fps and travels per animation
  animations/library.lock   # + NEW verdict on Fetched
  goldens/survivor/         # NEW  3 frames x 2 directions per clip, text
  staging/                  # gitignored: bare.glb, clean.glb, reports/
  preview/                  # gitignored: full size local scratch

project/assets/characters/survivor/
  sheet.png                 # NEW  committed contact sheet, LFS, by the atlas

crates/xtask-art/src/check/
  mod.rs                    # Finding, Severity, comparison, attempt, runner
  gltf_world.rs             # world transforms from the glTF node graph
  rig.rs                    # rig and object conformance, from [profile]
  mesh.rs                   # NEW  world, then weld, then count
  clip.rs                   # NEW  swing, twist, fps grid, object transform
  atlas.rs                  # pack and manifest invariants
  validator.rs              # NEW  runs npm gltf-validator, maps its report
crates/xtask-art/src/
  spec.rs                   # + Subject::cleanup, Subject::symmetry
  lock.rs                   # + content hashes, + Model in the version guard
  library.rs                # + Animation::source_fps, travels, + verdict
  providers/                # NEW  one module per vendor, constants inside
    mod.rs                  # NEW  the contract, in one doc comment
    openai.rs
    meshy.rs                # + pose_mode, + print/analyze, + model_url rig
    mixamo/
      mod.rs                # NEW  SITE_URL, shared by client and session
      client.rs             # + traveling export, + fps from source_fps
      session.rs            # the Chrome token reader, was chrome.rs
  http.rs                   # NEW  retry_after and the env backoff helpers
  stages.rs                 # + model downloads bare.glb, + fixer before rig
  blender.rs                # NEW  argv builder, runner, sentinel, diagnostics
  cli.rs                    # + the concept retry loop, - pause_for_review

tools/gltf_validator/
  validate.mjs              # NEW  drives the npm validator, which has no CLI

tools/blender/src/
  transfer.py               # NEW  pure maths: matrices in and out, no bpy
  plant.py                  # NEW  pure maths: contact detection, 2 bone IK
  findings.py               # NEW  the shared Finding record and JSON writer
  retarget_animation.py     # bpy glue only: import, map, transfer, export
  mesh_clean.py             # NEW  the fixer only. It measures nothing
  concept_check.py          # NEW  image checks before the Meshy call
  bake_sprites.py           # root motion in world space, fps from source_fps
```

`art/characters/<char>/model.glb` is the rigged, skinned file the rig stage
writes. `art/skeletons/humanoid.glb` is shared by every character on that
skeleton (`library.rs:127`), so the rig stage never writes it. It is promoted
only by the deliberate operation in `art/skeletons/README.md`.

## Specs & Standards

- **glTF 2.0, Skins and Animations.** Joint world transforms come from the
  node hierarchy, and sampler input times must be strictly increasing.
  `check/gltf_world.rs` walks the node graph from the specification rather
  than trusting an importer, and the same specification explains fact 8. glTF
  has no `quads` primitive mode, so the spec's `quads: true` cannot be
  verified from a delivered GLB and `mesh.quads` is `info`, never `error`.
- **Khronos glTF-Validator.** Every issue it reports at `Error` severity is an
  error for us. Non-zero exit is its contract and ours.
- **Godot `SkeletonProfileHumanoid`** for the rest rules: +Y from parent joint
  to child joint, +X bends the joint like a muscle contracting, the character
  faces one documented axis in **Right-Handed Y-up**, and there is no node
  transform (`research_humanoid_rig_standards.md:52`).
- **Mixamo and HumanIK bone names**, prefix stripped, spine bottom-up:
  `Hips`, `Spine`, `Spine1`, `Spine2`, `Neck`, `Head`, then
  `{Left,Right}{Shoulder,Arm,ForeArm,Hand}` and
  `{Left,Right}{UpLeg,Leg,Foot,ToeBase}`.
- **Swing-twist decomposition.** The twist about an axis comes from the
  quaternion's vector part projected onto that axis, per fact 19. `mathutils`
  `Quaternion.axis` is normalized and must not be used.
- **Foot contact thresholds**, from `research_art_pipeline_qa_systems.md:110`
  to `114`, stated at 180 cm scale and converted to a rate before use.
  Contact when toe height is under 3 cm and speed under 30 cm per second.
  Skate when a foot slides more than 2.5 cm while under 5 cm of height. Real
  mocap scores about 0.10 cm per frame, so zero is not the target. A majority
  vote over about 5 frames at 60 Hz removes noise, scaled to the clip's rate
  and forced odd.
- **Unity loop-match rules**, evaluated separately: root rotation must match
  at the seam, root Y must match, root XZ deliberately must not
  (`research_art_pipeline_qa_systems.md:129`).
- **Blender command line.** `--python-use-system-env` is mandatory, because it
  is what lets `PYTHONPATH` reach the embedded interpreter, which is how
  `framing` and `transfer` are importable (`stages.rs:453`). Argument order is
  a documented silent-failure mode, so the invocation lives in one tested
  function. `--python-exit-code` is defense in depth only, per fact 15.
- **Meshy API.** Facts 11 and 12 carry the parameters and their costs.
- **ASD-STE100** for this document and every new error message.

## Interfaces

**One file per skeleton** holds `[profile]`, `[aim_table]` and the role
tables. `check/rig.rs` reads `[profile]`, `transfer.py` reads `[aim_table]`,
and **every published limit in this design lives in `[profile]`**, including
the `clip.*` and `bake.*` ones. Adding a skeleton means adding a file.

**The aim table is validated, not just parsed**, because it is the single
source of every bone's constant offset and a sign error on one row produces a
confidently wrong clip. Three rules: every role in the convention table has a
row, and a missing row is an error rather than a fallback to the source's rest
pose. Mirror rows are exact reflections across the mirror plane. Each
prescribed aim sits within `max_bind_deviation_degrees` of **both** rigs' own
rest aim, so a table that describes neither rig fails to load.

**Every gate returns the same record.** Rust emits it, Python emits it from
inside Blender, and Rust parses it.

```jsonc
{
  "rule": "clip.swing",             // stable id, printed by --list-rules
  "severity": "error",              // error | warning | info | skipped
  "subject": "LeftHand",            // bone, file, frame, object or direction
  "measured": 52.9, "limit": 2.0,
  "comparison": "le",              // le | lt | eq | ge  -- REQUIRED
  "unit": "degrees",
  "attempt": 1,                    // which regeneration produced this
  "measured_on": "world space, aligned by seconds from clip start, frame 7",
  "message": "left hand swings 52.9 degrees from the source"
}
```

`comparison` is required and load-bearing. Without it, `cleanup_effective`
with `measured = after, limit = before` passes a fixer that changed nothing,
at 8 le 8.

Contract for callers:

- Exit code is non-zero if and only if at least one `error` is present.
- **The comparison decides the severity, never the caller.** A rule reports
  every subject it resolves: `error` when the comparison fails, `info` with
  the number when it holds, `skipped` when a spec field switched the rule off,
  `warning` when a remote call was unavailable. Findings are built through the
  `Rule` registry `--list-rules` prints, so a rule cannot report a limit, a
  unit or a space that the printed list does not carry.
- A missing input is an error, never a skip. A missing golden is an error. An
  unavailable remote call is a `warning`, never silence.
- `measured_on` is mandatory and names the space. A rule with no space fails
  its own unit test.
- **A gate never emits NaN.** Where a measurement is undefined, it reports the
  reason as an `error` with a stated message. `clip.twist` at 180 degrees of
  swing is the one case, and its message is
  `"swing is 180 degrees, twist undefined"`.
- The runner writes `art/staging/reports/<stage>.<item>.<attempt>.json` on
  every run, one stem per clip or character so nothing overwrites a sibling,
  so no retry overwrites the attempt before it.

**Published limits.** Every value lives in `[profile]`. Calibration assets and
measured values are in the Test Plan, once, so the two cannot drift.

| Rule | Limit | Comparison |
|---|---|---|
| the five `concept.*` rules | set by T11 from the four committed views, headroom recorded | le |
| `clip.swing`, `clip.twist` | set by T6 from the CMU cross-rig clip | le |
| `clip.root_travel` | 0.02 m per axis | le |
| `clip.floor_snap` | 5 mm | le |
| `clip.foot_contact.skate` | 2.5 cm at 180 cm scale | le |
| `clip.foot_contact.penetration` | 5 mm | le |
| `clip.foot_contact.plants` | 1 per foot per cycle | ge |
| `clip.fps_grid` | 1e-4 frame | le |
| `clip.loop` | 2.0 deg, today's `LOOP_TOLERANCE_DEG` | le |
| `source.traveling` | 0.02 m of hip travel, the threshold both ways | ge when `travels`, le when not |
| `rig.child_axis` | 2.0 deg | le |
| `rig.mirror_length`, `rig.mirror_direction` | 1.0 percent, 1.0 deg | le |
| `rig.humerus_angle` | 15 deg from the target of 40 | le |
| `rig.world_height` | 5 percent of `spec.subject.height_meters` | le |
| `rig.bind_deviation` | 75 deg | le |
| `rig.names_standard`, `bone_set`, `single_root`, `parents`, `facing`, `up_axis`, `object_transform` | 0 defective bones, and exactly 1 bone per declared name for `bone_set`. A count of defects has no tunable limit, so these are the one family whose limit is not a `[profile]` number | eq |
| `mesh.holes`, `islands`, `self_intersect`, `mirror`, `printability` | set by T3 from `bare.glb` | le |
| `mesh.non_manifold_post` | set by **T10**, the first task that produces `clean.glb` | le |
| `mesh.cleanup_effective` | the pre-fixer count, over holes, islands and self-intersections only | **lt** |
| `mesh.budget` | 300,000 tris, Meshy's stated rigging limit | le |
| `mesh.uv` | 0 coordinates outside [0,1] | eq |
| `bake.in_frame`, `bake.pivot` | 1 px of alpha inset, 1 px of ground-line drift | ge, le |
| `bake.forearm_roll` | 0.0 | eq |
| `bake.sampled_frames_are_keys` | 0 rendered frames that are not authored keys | eq |

**Retarget errors are typed values** (requirement 7), never printed strings:

```python
class TransferError(Exception):
    code: str      # "role_unmapped" | "aim_row_missing" | "bone_missing"
                   # | "swing_singular"
    subject: str   # the role or bone name
```

**The command surface** grows one verb, and the Blender invocation is built by
one tested function, never string concatenation, always in this order:

```bash
cargo art check --list-rules    # every rule id, limit, comparison, unit, space

blender --background --factory-startup --offline-mode --python-use-system-env
        --python-exit-code 1 --log-level debug --log-file <path>
        --python <script> -- <args>
```

The script writes a sentinel JSON from an `atexit` handler registered first,
so it runs last, and only when `sys.excepthook`, `sys.unraisablehook` and
`threading.excepthook` recorded no failure. Rust asserts it exists,
per fact 15.

## Existing Code & Reuse

**Kept untouched**, because the audit found each one sound:

- `bake_sprites.py` rendering, framing and camera, `Framing.ortho_scale` and
  `merged` included.
- The direction ring in `framing.py` and its cross-check against
  `pack.rs::direction_names`.
- `pack.rs`, the sprite manifest, and the `crates/sprites` format.
- `mixamo.rs` as a client. Only the export request and the fps source change.
- `library.rs`, `MotionSource`, `redistributable`, `LibraryLock`, `Fetched`.
- `chrome.rs`, `strip_animation.py`, and the one-triangle skin carrier.
- `SkeletonRoles` and role-based matching, which `[profile]` extends in place.
- `framing.py::sampled_frames`, which already rounds to integers (fact 6).
- The `sys.exit(1)` wrappers on every Blender script.
- `scale_translation` and `translation_scale` as operations. Only the metric
  feeding them changes, from total height to femur length.
- `ConfirmSpend`. It guards money and stays the one mid-run touchpoint.

**Extended:**

- `spec.rs` gains `cleanup` and `symmetry` on `Subject`.
- `library.rs` gains `Animation::source_fps`, `Animation::travels` and
  `Fetched::verdict`.
- `stages.rs` gains a download inside `model` and a fixer before `rig()`.
- `cli.rs` gains the concept retry loop and loses `should_pause`.
- `lock.rs` gains content hashes and adds `Model` to the version guard.
- `cargo art check`, today a spec validator, becomes the rule runner.

**Deleted, each with the reason:**

| Deleted | Why |
|---|---|
| `rebase_action`, `limb_corrections`, `rest_to_parent`, `sole_children` | ~470 lines become ~110 in `transfer.py`. `sole_children` is the source of the 52.9 and 66.5 degree wrists |
| `verify_retarget.py`, `LIMB_CHAIN`, `limb_mismatch` | blind to twist, and no terminal bone is ever measured (fact 4). `limb_mismatch` is orphaned with `LIMB_CHAIN` |
| `align_to_world` | it calls `transform_apply(rotation=True)` on a rig that owns an action, which requirement 1 forbids absolutely. Reading `matrix_world @ pose.matrix` makes it unnecessary and disarms the trap |
| `bone_directions`, `bind_pose_mismatch` and its call at `bake_sprites.py:633` | both read `tail_local`, the importer-invented 100x tails. `rig.child_axis` replaces them from joint positions in the glTF node graph |
| `loop_mismatch`, `report_loop` | `report_loop` warns and never refuses. `clip.loop` replaces it with a limit and a comparison |
| the `array_index == 2` branch in `strip_root_motion` | it assumes `Hips` local Z is world Z (fact 5) |
| twelve tests in `test_framing.py` | each asserts a function against itself. Two of them test `bind_pose_mismatch`, which goes with it |
| `apply_forearm_roll` | a hand-derived patch for a roll problem the new transfer removes. Deleted in T15, and until then `bake.forearm_roll` is an error rule, because it runs after `clip.swing` and would otherwise be invisible |
| `pause_for_review`, `should_pause` | the four prompts at Concept, Model, Bake and Pack are replaced by gates plus pull request approval (decision 13) |

## Logic

**The transfer.** Requirements 2 and 8, with the aim table applied as
described in the retargeter decision. Fact 19 is the whole reason the first
line reads the way it does:

```python
def swing_twist(q, axis, bone):
    """Split q about axis. Project the quaternion's VECTOR PART, never
    `q.axis`, which is normalized and drops sin(angle / 2). That error is
    worst NEAR IDENTITY (proof_swing_twist_vector_part.md).
    """
    proj = axis * Vector((q.x, q.y, q.z)).dot(axis)
    if abs(q.w) < 1e-9 and proj.length < 1e-9:   # 180 deg of swing
        raise TransferError(code="swing_singular", subject=bone)
    twist = Quaternion((q.w, *proj)).normalized()
    return q @ twist.inverted(), twist           # swing, twist

def reference_pose(rig, aim_table):
    """Aim each bone as the table says, keep the rig's OWN twist."""
    for bone in parents_first(rig):
        role = role_of(bone)
        if role is None:                        # head_end, headfront: not driven
            continue
        want = desired_local(bone, aim_table[role]).to_quaternion()
        swing, _ = swing_twist(want, Vector((0, 1, 0)), bone.name)
        bone.matrix_basis = swing.to_matrix().to_4x4()
    return {b.name: world(b) for b in rig.pose.bones}   # ref_world_ours/_src

def offsets(ref_world_ours, ref_world_src):
    """One rotation per role. Never fed the aim table."""
    return {r: ref_world_src[r].to_quaternion().inverted()
               @ ref_world_ours[r].to_quaternion()
            for r in ref_world_ours}

def local(bone, world_out, rest, parent_world_out):
    """L = (R_parent^-1 R_bone)^-1 @ M_parent^-1 @ M_bone. No depsgraph."""
    if parent(bone) is None:                    # the root lives in object space
        return rest[bone].inverted() @ world_out[bone]
    basis = rest[parent(bone)].inverted() @ rest[bone]
    return basis.inverted() @ parent_world_out.inverted() @ world_out[bone]
```

`check/clip.rs` runs the same split for `clip.twist`, on full animated world
rotations where 180 degrees of swing from rest **is** reachable. A gate cannot
raise, so there it reports `clip.twist` as an `error` with the message
`"swing is 180 degrees, twist undefined"` rather than normalizing a zero
quaternion into a NaN.

**Only the root gets location keys.** `transfer()` returns the source's world
matrix, whose translation column is the source's joint position. Writing that
to every bone drags our joints to Mixamo's and our rig inherits Mixamo's limb
lengths. Non-root bones therefore get rotation channels only, exactly as
retarget_bvh does with `trgMatrix.col[3] = srcMatrix.col[3]`. Object
transforms are never applied: world matrices are read as
`obj.matrix_world @ pose_bone.matrix` and the root is written back through
`obj.matrix_world.inverted()`.

**Root travel, a per-axis maximum, on the stripped copy.** An endpoint
difference cancels a symmetric excursion, which is the pattern this design
condemns. Sources are fetched traveling (decision 12), so at the retarget
boundary a correct clip travels 2.2 to 2.7 m and the rule runs at the bake
boundary. Below it, root scale and floor snap (requirement 4, techniques 32
and 37):

```python
first = world_head(hips, frames[0])
worst = [max(abs(world_head(hips, f)[i] - first[i]) for f in frames)
         for i in range(3)]
for axis, value in zip("xyz", worst):
    finding("clip.root_travel", measured=value, limit=0.02, comparison="le",
            subject=axis, measured_on="world space, after strip_root_motion")

ratio = femur_length(ours) / femur_length(theirs)   # not total height
for key in every_location_key(action):              # the same operation
    key.co[1] *= ratio
lift = -min(world_z(toe, f) for toe in TOES for f in frames)
offset_root_by(lift)
finding("clip.floor_snap", measured=abs(lift_after), limit=0.005,
        comparison="le", subject="lowest toe frame")
```

`clip.root_travel` runs after `strip_root_motion`, which pins the horizontal
channels (fact 5), so it can only ever read a residual. It is not a
substitute for `source.traveling`, which is why that rule is symmetric.

**Foot planting** (requirement 6). Thresholds are world-space meters scaled
from the published 180 cm reference, and the speed threshold is a rate, so it
means the same thing at 8 fps and 30 fps.

```python
scale = character_height_m / 1.80
speed = lambda f: step_xy(toe, f) * source_fps        # meters per second
contact = [f for f in frames
           if world_z(toe, f) < 0.03 * scale and speed(f) < 0.30 * scale]
width = max(3, round(5 * source_fps / 60) | 1)   # odd and >= 3, or no majority
contact = majority_vote(contact, width)
runs = consecutive(contact)
finding("clip.foot_contact.plants", measured=len(runs), limit=1,
        comparison="ge")
for run in runs:
    lock_xy(toe, run, ramp_in=2, ramp_out=2)          # two bone analytic IK
```

**The fps invariant** (requirement 3). `source_fps` is the clip's own rate,
new and distinct from `fps`, the sprite sampling rate (fact 6). It emits
Findings, not bare asserts, which would vanish under `python -O`.

```python
scene.render.fps = animation.source_fps
lo, hi = round(action.frame_range[0]), round(action.frame_range[1])
for t in key_times(action):                  # float32 seconds, so a tolerance
    finding("clip.fps_grid", measured=abs(t - round(t)), limit=1e-4,
            comparison="le", unit="frame", subject=f"key at {t}",
            measured_on=f"scene fps {scene.render.fps} = source_fps")
finding("clip.fps_grid.range", comparison="eq", limit=0,
        measured=int((scene.frame_start, scene.frame_end) != (lo, hi)),
        subject=f"render range {lo}..{hi}")
```

**There is no divisibility rule between the two rates.** The invariant wanted
is that every rendered frame is an authored key, and it already holds:
`framing.py::sampled_frames` rounds each sample to an integer and the new
transfer keys every frame. `bake.sampled_frames_are_keys` asserts it directly.
A `source_fps % fps == 0` rule would instead force walk_back from 20 to 15 and
run from 24 to 30, changing playback speed, which Out of Scope forbids.

**The cleanup, in world space.** Fact 13 is Meshy's extension deleting nothing
because it measured local volume against a world checker, and this asset
family carries a 100x node scale, so the transform comes first and every
constant is world-space meters.

```python
bm.transform(obj.matrix_world)                  # world space, once, up front
bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-5)      # meters
drop_objects_not_in(profile.meshes)
drop_islands_under(1e-6)                        # m3, about a 1 cm cube
fill_holes(bm)
if spec.symmetry:
    bpy.ops.mesh.symmetrize(direction="POSITIVE_X", threshold=0.001)
```

The fixer measures nothing, which is rule three's whole point.
`check/mesh.rs` measures `bare.glb` and `clean.glb`, each taken to world
through the glTF node chain and then welded, and reports the pair.

## Edge Cases & Constraints

- **A gate that measures the wrong representation gives precise, wrong
  numbers.** Seam splitting turned 171 holes into 13,368, so every rule states
  its space and is calibrated before its limit is trusted.
- **A normalized axis is not a vector part.** Fact 19 is the same class of
  fault one level down: the wrong projection is invisible on a large twist and
  worst near identity, which is why the split ships with two numeric
  known-answer tests rather than one comparison.
- **The weld distance is itself a parameter.** Too large and it merges
  distinct vertices, hiding holes. T3 reports a merge histogram and picks the
  plateau rather than assuming 1e-5.
- **Two tools agreeing is not evidence.** Our `bmesh` scan and Meshy's
  extension matched to the integer and both were wrong, sharing the input.
- **An empty set has no maximum worth trusting.** Zero plant runs is an error,
  not a skate of 0.0. An even majority window is the same shape of nothing,
  which is why the vote width is forced odd and at least 3.
- **`input_task_id` wins if both are sent.** The rigging body carries
  `model_url` alone once cleanup is on, or the cleanup is discarded.
- **Rigging by `model_url` demands +Z facing in glTF Y-up and a texture**, and
  states no body size limit. The survivor's GLB is about 5 MB, so about 6.7 MB
  base64. T10's acceptance is a real 5-credit call, with a short-lived upload
  as the named fallback if a data URI is rejected.
- **`print/analyze` cannot be re-run after the fixer**, because it needs a URL
  and `clean.glb` is local. This is rule three's one exemption.
- **Symmetrize mirrors UVs with the geometry**, so asymmetric texture detail
  is duplicated and flipped. Reviewed on the model contact sheet.
- **`transform_apply` on a rig that owns an action is forbidden**, with no
  allowlist, which is why `align_to_world` is deleted.
  `clip.object_transform` is defined as "every object matrix in the output GLB
  is identity" plus "rest bone matrices byte-match the committed rig", not as
  a magnitude test.
- **A `YZX` euler cannot separate twist near a Z component of 90 degrees**, and
  our `Hips` sits there. The quaternion split has one singularity, at 180
  degrees of swing, and it is handled rather than argued away: `transfer.py`
  raises `swing_singular`, and `check/clip.rs` reports an error, because 180
  degrees of swing from rest is reachable on animated data.
- **Blender 5.x removed `Action.fcurves` and `Bone.select`**, `wm.popup_menu`
  segfaults headless, and the glTF importer invents bone tails. Never read a
  tail.
- **`nla.bake` needs `visual_keying=True` explicitly.**
  `clear_constraints=True` does not imply it despite its tooltip, and
  constrained motion bakes to zero.
- **Meshy is non-deterministic** and exposes no seed, so committing the
  artifact as a checkpoint is the correct response. It refunds a technical
  failure, never a generation you dislike.
- **The concept stage is not free.** Three attempts is 2.40 USD of OpenAI
  images, `ConfirmSpend` quotes that once, and the cap is in the loop.

## Test Plan

Every rule ships all three columns in the same pull request. The **Negative**
column marks its kind: `[art]` is real broken art, which decision 11 prefers,
`[synth]` is a built asset, `[mut]` disables a step of the measurement, which
tests the metric rather than the gate. Every `mesh.*`, `rig.*`, `clip.*`,
`bake.*` and `atlas.*` row runs in Rust, so its negative control is a required
CI check with no Blender. `mesh.quads` and `source.posture` are `info` rules,
so they are calibrated and carry no negative, per the Terminology exemption.

| Rule | Positive | Negative fixture it must reject | Calibration |
|---|---|---|---|
| `concept.background_flat` | committed `front.png` | `[synth]` a 30 percent gradient added | measured on the four views in T11 |
| `concept.single_figure` | committed `front.png` | `[synth]` two figures side by side | measured in T11 |
| `concept.arm_gap` | committed `front.png` | `[synth]` arms painted onto the ribcage | measured in T11 |
| `concept.mirror` | committed `front.png` | `[synth]` one arm at 115 percent | measured in T11 |
| `concept.cross_view` | the four committed views | `[synth]` a side view scaled 8 percent | pairwise silhouette height and centroid, T11 |
| `mesh.printability` | recorded response for `bare.glb` | `[synth]` a response with `is_watertight: false` | `bare.glb` in T3. `model.glb` reads 179 combined edges |
| `mesh.holes` | `bare.glb`, world then welded | `[mut]` the same file with the weld step disabled, over 13,000 | T3 |
| `mesh.non_manifold_post` | `clean.glb` | `[synth]` 3 faces on one edge | **T10**, the first task that can produce `clean.glb`. On `model.glb` the fixer took 8 to 13 |
| `mesh.islands` | `bare.glb`, welded | `[synth]` a 5 mm cube welded into the file | T3 |
| `mesh.self_intersect` | `bare.glb`, welded, via `parry3d` | `[synth]` two interpenetrating cubes | T3. `model.glb` reads 16 |
| `mesh.mirror` | symmetrized `clean.glb`, 0.00 | `[art]` `bare.glb` before the fixer | T3. `model.glb` reads 3.02 percent |
| `mesh.world_size` | through the node chain, 1.70 m | `[mut]` read from the local bbox, 170 m | 1.70 against a spec of 1.7 |
| `mesh.facing` | `bare.glb`, +Z in glTF Y-up | `[synth]` the same mesh yawed **180**, facing away | toe-tip minus heel of the foot island, signed on Z |
| `mesh.stray_object` | `clean.glb`, only `char1` | `[synth]` a second mesh node. **Corrected in T2:** the committed `model.glb` at HEAD holds 26 nodes, one mesh, `char1`, and one primitive, so it carries no `Icosphere` and cannot serve. T3 measures `bare.glb`, where the debris is | the `meshes` allowlist |
| `mesh.budget` | `bare.glb` tris under 300,000 | `[synth]` 400,000 tris | `model.glb` reads 54,864 tris against the spec's 30,000 target. `bare.glb` in T3 |
| `mesh.uv` | `bare.glb`, one tile in [0,1] | `[synth]` a UV at 1.4 | 0 out-of-bounds on `model.glb` |
| `mesh.texture` | `bare.glb` has a base color image | `[synth]` the same GLB with materials stripped | rigging's own precondition |
| `mesh.cleanup_effective` | a real run, holes 171 to 27 | `[synth]` a fixer stub that changes nothing, 8 lt 8 fails | the before and after pair. Non-manifold is excluded and owned by `non_manifold_post` |
| `rig.bone_set` | `[synth]` a conformant rig, because the rename lands in T5 | `[art]` the current rig, which is missing `Spine1`, `Spine2` and `Neck`. Measured: 3 of 24 names absent | the 24 committed names, each present exactly once |
| `rig.parents` | `[synth]` a conformant rig | `[art]` the current rig, `Spine` above `Spine02`. Measured: 4 bones hang wrong, `Spine`, `Head`, and both shoulders | the committed parent map |
| `rig.single_root` | `humanoid.glb`, where every joint descends from `Hips` | `[synth]` `LeftUpLeg` parented outside `Hips`, which takes its whole branch with it, 4 bones | `humanoid.glb` |
| `rig.child_axis` | synthetic conformant rig | `[art]` the current rig, whose `Hips` +Y points out of the left hip. Measured: `Hips` 97.6 deg and `Head` 26.0 deg, every other bone under 0.01 | per bone, from joint positions. `[synth]` a joint moved without its parent, for the metric itself |
| `rig.mirror_length`, `rig.mirror_direction` | `[synth]` an exact mirror | `[art]` the current rig, worst 3.68 percent and 2.16 deg, both on the `Foot` segment. The 1.45 deg the audit gave is the `ForeArm` pair | measured now: length 0.95 to 3.68 percent over six segments a side, direction 0.08 to 2.16 deg |
| `rig.world_height` | `humanoid.glb`, 2.05 percent out, confirmed at 2.049 | `[synth]` a rig scaled by 100 at the object node, which reads 9900 percent and trips nothing else | 5 percent tolerance, against `spec.subject.height_meters` |
| `rig.humerus_angle` | a conformant rig at 40 | `[art]` the current rig at 59.1 left and 59.3 right, 19.1 and 19.4 out | target 40, tolerance 15 |
| `rig.bind_deviation` | `humanoid.glb`, worst 4.1 deg up the root chain | `[synth]` a rig rolled 90 deg onto its side | the 75 deg band. Measured per step of the root chain, against the up axis, which is the only canonical direction that exists before the aim table |
| `rig.facing` | the current rig already faces +Z in glTF, which is minus Y in Blender, 8.1 to 9.0 deg per foot | `[synth]` the rig yawed 180, and `[synth]` a foot pointing along the up axis, where the facing is undefined and reported as one | the bake camera's forward. Per foot, so one foot on backwards cannot average away |
| `rig.up_axis` | `humanoid.glb`, 3.0 deg off Blender +Z | `[synth]` the same rig pitched 90 deg, which is the shape of an export with no axis conversion | `humanoid.glb`. The root chain end to end, named by the closest of six axes |
| `rig.object_transform` | `humanoid.glb` and `model.glb`, whose bones carry a bind-pose action while the object carries none | `[synth]` a rig carrying an action on the object | both committed rigs read 0 channels above the skeleton |
| `rig.names_standard` | `[synth]` a conformant rig | `[art]` the current rig, which names `Spine01`, `Spine02` and `neck` | the Mixamo name list. This rule reports the names nothing can map, and `bone_set` reports the roles that are missing |
| `gltf.validator` | every shipped GLB | `[synth]` a GLB with an injected NaN | the four committed GLBs |
| `source.posture` | `strafe_left.fbx`, head 34 to 37 | none, `info` only | the three Mixamo clips |
| `source.fps_declared` | `source_fps` equals the file's rate | `[synth]` a `library.ron` with `source_fps` 24 against a 30 fps FBX | the three clips, read from the FBX |
| `source.traveling` | `strafe_left.fbx` at `travels: true`, 2.31 m of hip travel, and `idle` at `travels: false`, under 2 cm | `[synth]` **both directions**: an in-place export declared `travels: true`, and a traveling export declared `travels: false` | symmetric on 0.02 m, so a mistyped flag fails either way. `travels` is declared per clip in T7 |
| `clip.swing` | the new output | `[art]` the shipped `strafe_left.glb`, whose wrists are 52.9 and 66.5 deg out of their own rest. The absolute cross-rig figure is measured in T6 | the CMU cross-rig clip, T6 |
| `clip.twist` | the new output | `[synth]` the new output with a 90 deg twist **post-multiplied in the bone's local frame**, `q @ Quaternion((0, 1, 0), radians(90))`, on `LeftUpLeg`. The same test asserts `clip.swing` stays under its limit, which is what proves the injection is a twist and not a yaw. Pre-multiplying by a world +Y rotation would yaw a downward thigh and fire `clip.swing` instead. The shipped clips cannot serve: `rotation_difference` is pure swing, so they carry our rest twist unchanged (fact 2) | the CMU cross-rig clip, T6 |
| `clip.fps_grid` | a clip at its own `source_fps` | `[art]` the shipped `strafe_left.glb` in a 24 fps scene, range 0.8 to 16.8 | `run.glb` |
| `clip.object_transform` | the new transfer | `[mut]` a variant calling `transform_apply(scale=True)` | object matrices identity on `run.glb` |
| `clip.root_travel` | the new output, under 2 cm | `[art]` the shipped `strafe_left.glb`, 0.315 m on Z after strip | `run.glb` |
| `clip.floor_snap` | the new output | `[synth]` the same output with the snap step removed | the lowest toe frame |
| `clip.foot_contact.plants` | the new output | `[synth]` an in-place clip, which yields zero contacts | `ge 1` per foot per cycle |
| `clip.foot_contact.skate` | the new output | `[synth]` a clip whose planted foot is translated 5 cm during stance | real mocap 0.10 cm per frame |
| `clip.foot_contact.penetration` | the new output | `[synth]` the root lowered 2 cm | 5 mm |
| `clip.loop` | `run.glb` | `[synth]` a clip cut one frame short | `idle.glb` and `run.glb`, 2.0 deg today |
| `clip.interpolation`, `clip.reference_pose_key` | all LINEAR, no frame 0 key | `[synth]` an action left on Bezier with a reference-pose key | `run.glb` |
| `bake.frame_count`, `bake.non_empty` | the rendered set | `[synth]` one frame deleted, and one fully transparent | directions x sampled frames, alpha coverage |
| `bake.in_frame`, `bake.pivot` | the rendered set | `[synth]` a pose clipped at the border, and a frame offset 20 px | 1 px inset, and the ground line across directions |
| `bake.forearm_roll` | spec field 0.0 | `[synth]` the field set to 30 | `eq 0` |
| `bake.sampled_frames_are_keys` | the rendered set on `run.glb` | `[synth]` an action with every other key deleted | `framing.py:260` already rounds to integers |
| `bake.landmark_golden` | committed golden | `[synth]` one arm rotated 30 deg before projection | 3 frames x 2 directions per clip |
| `atlas.frame_count`, `atlas.trim_boxes` | the packed atlas | `[synth]` a manifest claiming one extra frame, and a box one pixel outside | the committed atlases |
| `atlas.manifest_schema` | the packed manifest | `[synth]` a missing field | the committed manifests |

**Unit, `pytest`, no Blender, in CI.** Coverage at 100 percent for `framing`,
`transfer`, `plant`, `concept_check` and `findings`, all `bpy` free.

- **Known-answer tests, expected values written as numbers.** Two or three
  poses whose world joint positions a human computed once, as
  `data/known_poses.json`. Plus three on the split and the offset, none of
  which compares against the code under test:
    - a pure 10 degree twist about +Y splits into **twist 10.00 degrees and
      swing 0.00 degrees**. The `q.axis` form gives 90.22 and 80.22
      (`proof_swing_twist_vector_part.md`), so this test alone rejects it.
    - a pure 30 degree swing about +X splits into **swing 30.00 degrees and
      twist 0.00 degrees**.
    - `Offset(LeftUpLeg)` is a pure twist of about **174 degrees**, never
      identity. This one passes under either form, which is why the two above
      exist.
- **The singular case has a stated answer.** A quaternion that is 180 degrees
  of swing about an axis perpendicular to +Y raises
  `TransferError { code: "swing_singular" }` from `transfer.py`, and
  `check/clip.rs` emits `clip.twist` as an `error` carrying
  `"swing is 180 degrees, twist undefined"`. Neither may return NaN, and
  neither assertion reads the split's own output.
- **Roleless bones.** A fixture rig carrying `head_end` is skipped, not raised
  on, and a missing required role raises `role_unmapped`.
- **Negative controls on `transfer.py`:** offset applied on the wrong side,
  composition order reversed, left and right roles swapped, the aim table fed
  to `offsets()` directly, a bone silently dropped, an aim row missing, a sign
  error on one mirror row.
- **Metamorphic relations.** Rotate the source 90 degrees about world up and
  every output orientation rotates by exactly 90. Mirror the source and left
  and right trajectories swap. Time-reverse the source and the output
  time-reverses. Scale the source rig by 1.5 and **output bone lengths are
  identical**, the control for rotation-only keys. A consistently wrong
  transform cannot satisfy these, and a round trip can, so there is none here.
- **Aim table validation:** a missing row, a broken mirror pair, and an aim
  outside `max_bind_deviation_degrees` of either rig.
- **Foot planting** in `plant.py`: a toe path with two known plant runs, a
  path with none, the same path at 8 and 30 fps giving the same runs, and the
  vote width odd and at least 3 at both rates.

**Unit, `cargo nextest --test unit`, in CI.** Every row above, plus:

- `check/gltf_world.rs` against hand-written fixtures with a nested hierarchy
  and a non-identity parent scale.
- `check/clip.rs`'s split against the same two numeric cases as the Python
  side, so the two implementations are pinned to one external answer.
- The `print/analyze` parser against a recorded response.
- The rigging body: `model_url` present and `input_task_id` absent when
  `cleanup` is on, and the reverse when off.
- The Blender argv builder: exact flag order, and a test that fails if
  `--python-exit-code` or `--python-use-system-env` is missing.
- A lint test that fails if `transform_apply` appears anywhere in
  `tools/blender/src/`.
- The concept retry loop: three failures leave three numbered reports and
  bail, a pass on attempt two proceeds **and the fixture asserts attempt two
  called `concept` with `force = true`**, and no Meshy stage is ever wrapped.
- Lock: one byte of `humanoid.glb` invalidates retarget and bake but **not**
  `Rig` or `Model`. One byte of a concept PNG invalidates `Model`. Editing one
  `[aim_table]` row invalidates every `Fetched` record in the library lock,
  which is where the retarget runs (fact 18), and the character lock's `Bake`,
  which is what consumes the two committed Meshy clips.
- CI asserts `MARROWFALL_UPDATE_GOLDENS` is unset.

**Integration, `cargo nextest --test integration`:** `cargo art check` on the
committed art produces the same report twice. A stage that fails a gate leaves
the report and does not advance the lock. With the network unavailable,
`print/analyze` reports `warning` and the build continues. With
`symmetry: false` the mirror rules report `skipped` and their negative
controls still run.

**End to end, `cargo nextest --test e2e`:** T16 launches Godot headless, loads
every atlas and manifest, and greps the log, because Godot exits 0 on a script
error (`research_godot_ci_e2e_testing.md`). Loading only, never pixels.

**Cross-reference to Out of Scope:** no test asserts color pixels, loads a
quadruped profile, calls Meshy's paid repair, checks a concept arm angle,
changes a sprite rate, or asserts a strict T-pose bind.

## Documentation Changes

- `art/skeletons/README.md`: `[profile]`, `[aim_table]`, the new bone names,
  and the "regenerate in one deliberate operation" sequence, which is also the
  only way `humanoid.glb` is promoted.
- `art/characters/README.md`: `model.glb` is the rigged, skinned file the rig
  stage writes, and `art/staging/<char>/bare.glb` is the mesh before rigging.
- `crates/xtask-art/README.md`: the `model` stage now downloads and cleans,
  `check` is a new verb, the free and paid split changes, the concept retry
  loop is documented, and the stale `art/pipeline/` reference goes.
- `README.md`: `cargo art check`, the two new spec fields, one line saying
  gates run at stage boundaries, and the E2E tier row changes from "nothing
  yet" to `render`.
- `tools/blender/README.md` (new, short): why `transfer.py` and `plant.py`
  never import `bpy`, why the fixer measures nothing, and the four rules.
- `docs/research/agent_reports/audit_the_current_art_pipeline.md`: a
  correction note. Its mesh numbers are the seam-split reading.

## Development Environment Changes

- Bun added to the `Brewfile` and to `setup`, with the npm `gltf-validator`
  pinned in `bun.lock`. There is no Homebrew formula for the validator and the
  published binaries are x64 (fact 16). Bun installs it and runs it, so the
  repository needs one JavaScript runtime rather than a runtime plus npm.
- `opencv-python-headless` added to the `uv` dependencies.
- `gltf` and `parry3d` crates added to `crates/xtask-art/Cargo.toml`.
- `pyproject.toml`: `--cov=framing,transfer,plant,concept_check,findings` with
  `--cov-fail-under=100`. All five are `bpy` free.
- `MARROWFALL_UPDATE_GOLDENS`, unset by default. Set to `1` a golden is
  rewritten, and CI asserts it is unset.
- **Failure diagnostics**, per `research_unattended_art_pipelines.md:181`,
  owned by T1: the exact `blender` argv written verbatim to a file beside the report,
  `--log-file` at debug level beside it, the `.blend` saved from the exception
  handler, and partial frames kept on a render failure. All under
  `art/staging/`, gitignored.
- `.gitignore` needs no change. `/art/staging/` and `/art/preview/` are
  already ignored, `art/goldens/` is text, and the contact sheet lives under
  `project/assets/characters/<char>/`, which `.gitattributes` already routes
  to Git LFS.
- `pr.yml`: add `tools/blender/**` to the `python` filter, make the `required`
  aggregator the single required check pinned by `app_id`, empty the bypass
  list, and enable `enforce_admins` and `require_last_push_approval`.
- No new CI runner. Nothing added to CI needs Blender or a GPU.

## Tasks

Sixteen vertical slices, 28.0 engineer days. Per decision 11 no gate is a
required CI check until T15, the pull request that regenerates the survivor
through the full gated pipeline. Until then each gate ships as a test whose
negative fixture is the current art, so nothing is ever red and nothing is
waived.

```text
T1 harness + validator + diagnostics
  ├─▶ T2 rig check ──┬─▶ T3 mesh check ──▶ T10 fixer ──▶ T11 concept + retry
  │                  └─▶ T4 aim table
  └─▶ T13 lock

T4 ──▶ T5 transfer + rename + refit ──┬─▶ T6 clip.swing / clip.twist
                                      └─▶ T7 source_fps + travels + travel
T7 ──▶ T8 femur + floor ──▶ T9 planting
T2 + T3 ──▶ T12 pose_mode spike
T1 + T6 + T7 ──▶ T14 bake, atlas, sheet, goldens

T5,T6,T7,T8,T9,T10,T11,T12,T13,T14 ──▶ T15 regenerate + gates required ──▶ T16
```

| #   | Task | Cost | Description | Success Criteria | Deps |
| --- | ---- | ---- | ----------- | ---------------- | ---- |
| T1  | Harness, Finding, validator, diagnostics | 1.0 d | One tested function builds every `blender` argv in the documented order, `--python-use-system-env` and `--log-file` included. `findings.py` and `check/mod.rs` carry `Finding` with the required `comparison`, `attempt` and `measured_on`, writing `reports/<stage>.<item>.<attempt>.json`. Each script writes a sentinel Rust asserts. `check/validator.rs` runs the npm validator under Bun. Diagnostics: argv verbatim, the log, the `.blend` from the exception handler, partial frames. | A unit test fails if any flag or the order changes. A script raising outside its top level leaves no sentinel and fails. A rule with no `measured_on` or no `comparison` fails its own test. No gate can emit NaN. An injected NaN fails and the four committed GLBs pass. A deliberately failed bake leaves argv, log, `.blend` and partial frames. | none |
| T1b | Isolate vendors | 1 d | From the human's review. Move `openai.rs`, `meshy.rs`, `mixamo.rs` and `chrome.rs` under `providers/`, with `chrome.rs` becoming `mixamo/session.rs`. Vendor URLs, ids and keys live only in their own module; `cli.rs` imports them. Generic HTTP helpers (`retry_after`, the env backoff) move to `http.rs`. `stages.rs` and `cli.rs` are the only callers, and no new one is added. No new traits: `MotionSource` in `library.rs` is already the seam. | A unit test greps `src/` and fails on any vendor host, id or key outside `providers/`. Scoped to `src/` because a test keeps its own copy of a literal on purpose: `test_mixamo_session.rs` rebuilds the Local Storage key from the origin, and importing `SITE_URL` there would move both sides together and the test could no longer fail. Every existing test passes unchanged. `cargo doc` links resolve. | T1 |
| T2  | Skeleton profile, rig and object check | 2 d | `[profile]` in `humanoid.toml`, `[profile.tails]` included. `check/gltf_world.rs`, `check/profile.rs` and `check/rig.rs` in Rust. Thirteen `rig.*` rules including `humerus_angle`, `facing`, `up_axis`, `bind_deviation` and `child_axis`, plus one height rule against `spec.subject.height_meters`. A `Rule` registry in `check/mod.rs` that every finding is built through, so `--list-rules` cannot advertise a limit a rule does not use. `spec.name` refused at load time when it holds a dot. | Every `rig.*` row rejects its negative fixture, seven of them being the current committed rig, on 22 subjects. Runs in CI with no Blender. `cargo art check` prints the itemized defect list. Every rule reports on good art too, at `info`, because a rule that goes quiet when it passes cannot be told from one that never ran, and `skipped` is added beside it so T10's switched-off rules are not the same word. | T1, T1b |
| T3  | Mesh measurement in Rust, calibrated on `bare.glb` | 2 d | **First: download `bare.glb`, take it to world through the glTF node chain, then weld.** Report a merge histogram at 1e-6, 1e-5, 1e-4 and 1e-3 and pick the plateau. Then `check/mesh.rs`: holes, non-manifold, islands, self-intersections via `parry3d`'s `Bvh`, mirror distance, world size, budget, UV bounds, facing, texture. Map `print/analyze` into the report. Write every `bare.glb` limit into `[profile]`. | The weld distance is chosen from the histogram, not assumed. Every `bare.glb` limit in `[profile]` is a real number. Every `mesh.*` row except `non_manifold_post` rejects its negative fixture in CI with no Blender. | T1, T2 |
| T4  | Aim table and role map | 1.5 d | Add `parents`, `optional`, `fingerprint` and `[aim_table]`. Implement the three table validations and the skip-unmapped-ancestor parent walk. Checked against a synthetic fixture, and the real rig is renamed in T5. | No second copy of the map exists. A missing row, a broken mirror pair, and an out-of-band aim each fail to load. | T2 |
| T5  | The transfer, the rename, and the refit | 3.5 d | `transfer.py` with no `bpy`: quaternion swing-twist aim application projecting the **vector part** per fact 19, `swing_singular` raised at 180 degrees, roleless bones skipped, separate `ref_world_*` dicts, algebraic local matrices, rotation-only keys for non-root bones, typed errors, LINEAR and CONSTANT, no reference-frame key. Delete `rebase_action`, `sole_children`, `align_to_world`, `bind_pose_mismatch`, `bone_directions` and the twelve self-referential tests. **Rename the committed `humanoid.glb` bones and refit `idle.glb` and `run.glb` in this PR**, because the rename invalidates them and this is the first task with the new retarget. | The three numeric known-answer tests pass: 10 deg twist to 10.00 and 0.00, 30 deg swing to 0.00 and 30.00, `Offset(LeftUpLeg)` about 174 and not identity. The 180 degree case raises `swing_singular`. A rig with `head_end` is skipped, not raised on. Requirements 1, 2, 7, 8, 9, 10 each have a passing test and a rejected negative. The 1.5x scale test gives identical output bone lengths. | T1, T4 |
| T6  | Clip verifier: swing and twist | 1.5 d | `check/clip.rs`: `clip.swing` absolute against the vendor file, `clip.twist` as the change from each rig's own rest twist about its own +Y using the same vector-part split, both over every mapped bone, frames aligned by seconds. `clip.object_transform` as identity object matrices plus byte-matching rest bones. Commit one CMU BVH clip and **measure both limits from it**. Delete `verify_retarget.py` and `LIMB_CHAIN`. | Both limits are written into `[profile]` from a measurement, not assumed. The Rust split passes the same two numeric cases as the Python one. `clip.swing` rejects the shipped `strafe_left.glb` and stays quiet on `run.glb` and the CMU clip. `clip.twist` rejects the post-multiplied 90 deg twist, and `clip.swing` stays under its limit on that same fixture. A 180 degree swing reports an error, never a NaN. | T5 |
| T7  | `source_fps`, `travels`, traveling fetch, root travel | 1.5 d | Add `Animation::source_fps` and `Animation::travels`, filling `source_fps` from each vendor file and **declaring `travels` for every clip**: `false` for `idle`, measured for `run` because a Meshy library clip is likely in place, `true` for the three Mixamo clips. Scene fps equals `source_fps`, with the key grid and range asserted as Findings. Request traveling export from Mixamo and add `source.traveling`, **symmetric on 0.02 m of hip travel in both directions**. Move `clip.root_travel` to the bake boundary as a per-axis maximum on the stripped copy. Delete the `array_index == 2` branch, `loop_mismatch` and `report_loop`, and add `clip.loop`. | Requirement 3 holds and the 0.8 to 16.8 fixture is rejected. `source.traveling` rejects an in-place export declared `travels: true` **and** a traveling export declared `travels: false`, so a mistyped flag cannot skip the gate. `run`'s `travels` is a recorded measurement, not a default. Root travel after strip is under 2 cm on all three axes. No sprite rate changes. | T5 |
| T8  | Femur scale and floor snap | 1.5 d | Femur ratio replaces total height, with every location key scaled in the same operation. Snap the lowest foot frame to Z equals 0 and report `clip.floor_snap`. | Requirement 4 holds. Travel matches the source within 2 percent. `clip.floor_snap` is under 5 mm, and the fixture with the snap removed is rejected. | T7 |
| T9  | Foot planting | 3 d | `plant.py`: contact detection at the published thresholds, scaled to character height and expressed as a rate, a majority vote whose width is odd and at least 3, foot XY lock, two bone analytic IK, ramps. | All three `foot_contact` sub-rules have a row and a rejected negative. Plants is an error at zero runs. Skate under 2.5 cm and penetration under 5 mm on every clip. The same toe path at 8 and 30 fps gives the same runs, and the vote width is odd and at least 3 at both rates. | T8 |
| T10 | Cleanup, symmetrize, and the post-cleanup ceiling | 2 d | `Subject::cleanup` and `Subject::symmetry`, false by default, true for the survivor. `mesh_clean.py` in world space, measuring nothing. The `model` stage downloads `bare.glb`. Rigging sends `model_url` as a data URI with `input_task_id` omitted. **Measure the first real `clean.glb` and write `mesh.non_manifold_post`'s ceiling into `[profile]`**, because T3 has no `clean.glb` to read. | **A real 5-credit rigging call with a data URI succeeds**, or the short-lived upload fallback ships instead. `cleanup_effective` shows holes, islands and self-intersections strictly decreasing. `non_manifold_post`'s ceiling is a measured number and every later run stays inside it. Texture and UVs survive. The no-op stub is rejected at 8 lt 8. With `symmetry: false` the fixer skips it, the mirror rules report `skipped`, and their negative controls still run. | T3 |
| T11 | Concept gates, calibration and the retry loop | 1.5 d | **First: measure the five `concept.*` rules on the four committed views and write the limits with their headroom into `[profile]`.** Then `concept_check.py`: background, one figure, arm gaps, mirrored silhouette when `symmetry` is on, and `cross_view` across the four views. Then the retry loop in `cli.rs`, three attempts total, `force = true`, numbered reports. Delete `pause_for_review` and `should_pause`. | No `concept.*` threshold is guessed. Each rule rejects its negative fixture. Three failures leave three numbered reports and bail with the images on disk. A pass on attempt two proceeds, and the test asserts attempt two called `concept` with `force = true`. `ConfirmSpend` quotes 2.40 USD once. No Meshy stage is wrapped. | T1, T10 |
| T12 | `pose_mode` spike | 0.5 d, 90 credits | Step 0: a free unknown-parameter probe. Then regenerate the model stage three times, unset, `"a-pose"`, `"t-pose"`, running every `mesh.*` and `rig.*` gate on each. | The four acceptance items are each answered with a number and a committed contact sheet. One value is adopted into the request body, or the field stays unset with the measurement recorded. | T2, T3 |
| T13 | Lock fingerprints real inputs | 1 d | Hash `humanoid.glb`, `humanoid.toml`, the concept PNGs, every animation GLB, `pose_mode`, the Blender version and the script version into the right stages. Add `Model` to the version guard. Add `verdict` to `Fetched`. | `humanoid.glb` invalidates retarget and bake but not `Rig` or `Model`, so a local rename spends nothing. A concept PNG invalidates `Model`. An `[aim_table]` row invalidates every `Fetched` record and the character lock's `Bake`. A Mixamo clip's verdict is readable in `library.lock`. | T1 |
| T14 | Bake and atlas gates, sheet, goldens | 2 d | Seven `bake.*` rules including `sampled_frames_are_keys`, and three `atlas.*` rules. Commit the downscaled contact sheet under `project/assets/characters/<char>/` and upload the full one as a CI artifact. Landmark goldens, 3 frames by 2 directions per clip. Route the clip audition into the fetch report. | Every `bake.*` and `atlas.*` row rejects its negative fixture. A wrong arm shows as a changed number in the diff. A missing golden fails. The audition numbers survive an unattended run in `reports/fetch.1.json`. CI asserts `MARROWFALL_UPDATE_GOLDENS` is unset. | T1, T6, T7 |
| T15 | Regenerate the survivor, flip gates to required | 1.5 d, ~35 credits | One deliberate operation on the `bare.glb` **that T12's winner produced**: clean, symmetrize, re-rig, promote to `art/skeletons/humanoid.glb` per its README, refit `idle.glb` and `run.glb`, refetch the three Mixamo clips traveling, re-bake, re-pack, re-golden, re-sheet. Delete `apply_forearm_roll`. Then make the `required` aggregator the single required check, pinned by `app_id`. **Regenerate a second time if the first pass teaches something.** | Every gate passes on the regenerated art with zero waivers. Cost recorded: paid is rigging 5 credits plus image-to-3d 20 to 30 only if T12 adopted a `pose_mode`, about 0.45 USD at 0.013 per credit. Free is `print/analyze`, the cleanup, the Mixamo refetch, the retarget, the bake, the pack and the goldens. `model.glb`, `humanoid.glb`, `idle.glb`, `run.glb`, every atlas under `project/assets/characters/` and the sheet move in one PR. | T5, T6, T7, T8, T9, T10, T11, T12, T13, T14 |
| T16 | Godot e2e smoke test | 2 d | Fill the empty e2e tier: launch Godot headless, load every atlas and manifest, grep the log for `SCRIPT ERROR`, a load failure and a leaked object. Add the `pkill` watchdog, because Godot hangs rather than exits on a fatal error. | A deliberately corrupted manifest fails the test. Headless loads only, never pixels. The `README.md` tier table names `render`. | T14, T15 |
