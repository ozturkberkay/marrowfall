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

This document runs about 2,050 lines against the template's stated 1,000. The
overrun is the 53-row test table that decision 11 makes the contract, the
20-row facts table that fourteen sections cite by number, one question per
settled decision, and the corrections each task makes to it. Reaching 1,000
means deleting one of those.

## In Scope

- One written skeleton spec, stored as data, plus a check that says which
  rules a rig breaks and by how much, and a rename to the standard names.
- An in-house retarget that transfers motion in world space against an
  absolute per role aim table, replacing the current local-space math.
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
  the framing math.** The audit found them sound, so sprite rates and
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
- **Self-intersection:** a pair of triangles from the same mesh that meet
  without sharing a vertex or an edge. Counted as faces. **Meet, not
  penetrate:** two triangles have no thickness, so a depth test reports 0 on
  a real crossing, and after a weld two faces that meet and share no vertex
  are two different parts of one surface in contact.
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
  its measurement, and `info` means measured and not a defect, inside its
  limit. A rule that goes quiet when it passes cannot be told from a rule
  that never ran. Both kinds live in one report, so a consumer reads the
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
| 8 | Mesh topology, one file, three states: raw glTF 13,368 boundary edges, welded 171, welded and cleaned 27. Welded non-manifold 8, 27,508 vertices. Before welding: 34,490 vertices and 54,864 tris. Islands: 7 inside `char1`, plus the stray `Icosphere` as a separate object. Every figure is `model.glb`. **T3 re-measured all of these in Rust and reproduced every one exactly**, and it corrects two: `model.glb` at HEAD holds no `Icosphere`, so the island count is 7, and the self-intersection figure of 16 could not be reproduced by any threshold (T3 reads 861 faces meeting, 41 penetrating). The unwelded reading is 413 islands and **0** non-manifold edges, because a seam splits every edge that would have been shared. | `research_meshy_mesh_repair_options.md:22`, `research_concept_and_model_stage.md:37`, `check/mesh.rs` |
| 9 | Mesh mirror asymmetry on `model.glb`: mean 0.50, p99 1.76, max 3.02 percent of width, by an independent kd-tree pass. **T3's own BVH pass reads mean 0.490, p99 1.659, max 3.021**, so the two implementations agree on the worst case to three decimals. `symmetrize` ran in 55 ms and drove it to 0.0. `symmetry_mode` is deprecated. | `research_concept_and_model_stage.md:72`, `check/mesh.rs` |
| 10 | `art/characters/survivor/model.glb` is already the rigged, skinned file. Cleaning geometry there desyncs skin weights: a trial changed vertex count by minus 23. T3 calibrated on `model.glb` welded and marked every `mesh.*` limit provisional, because no key here could download `bare.glb`. **T12 closed that**: the survivor's own paid task answers 404 under the key that works, so it regenerated the bare mesh from the same four views and read every row off it. The real bare mesh is **1.897 m tall, centered on the origin, and its one mesh node has no name**: `height_meters` and `char1` are both created by the rigging call. | `research_meshy_mesh_repair_options.md:59` |
| 11 | `POST /print/analyze` costs **0 credits**. `POST /print/repair` is 10 credits and **strips textures**, and rigging refuses an untextured mesh. Meshy credits run about 0.013 USD each, from 20 to 30 credits at 0.25 to 0.40 USD. | Meshy API reference, `research_concept_and_model_stage.md:23` |
| 12 | Rigging accepts `model_url` as a **URL or data URI** of a textured `.glb`, costs 5 credits, and `input_task_id` wins if both are sent. With `model_url` the character must face **+Z in glTF Y-up**. No body size limit is stated. The face limit of 300,000 is stated for `input_task_id`. | `reference.md:643,644,651` |
| 13 | Meshy's extension returns `FINISHED` from `delete_small_pieces` having deleted nothing, because it measures piece volume in **local** space while its checker measures in **world** space. | `research_meshy_mesh_repair_options.md:54` |
| 14 | `lock.rs` fingerprints no art file, and `LOCAL_PIPELINE_VERSION` covers only `Bake` and `Pack`. The rig can be replaced and every stage reports cached. `Stage::Concept` bills OpenAI, so `costs_credits()` is already true for it. **T13 closed this**: every stage now fingerprints the content of the files it opens, `Model` joined the version guard, and `Stage::is_versioned` is where that set lives. | `lock.rs:72,237,243` |
| 15 | `--python-exit-code` catches only top-level exceptions. Raised from a `bpy.app.handlers` callback, `atexit`, `unregister` or a thread, Blender **exits 0**, so the success sentinel is the real gate. | `proof_python_exit_code_coverage.md` |
| 16 | Blender ships no official Linux arm64 build and our CI runner is `ubuntu-24.04-arm`, so nothing in CI can call `bpy`. The npm `gltf-validator` is Dart compiled to JS, so it is architecture independent. | blender.org, `pr.yml:18`, Khronos npm README |
| 17 | The hunched posture is in the Mixamo source: `strafe_left` is authored with the head 34 to 37 deg forward. Our output copies it faithfully. | audit |
| 18 | `stages::concept` reuses images on disk unless `force` is true, and `stages::retarget` is called from exactly one place, the Mixamo fetch path. | `stages.rs:35`, `cli.rs:459` |
| 20 | **A skinned mesh is placed by its joints, not by its node.** The glTF specification says the node transform of a skinned mesh must be ignored. `model.glb` is skinned under a 0.01 node with 100x joint translations, so composing the node chain anyway measures a 1.70 m character at **0.017 m**, and every absolute constant then reads 100x wrong: `parry3d`'s f32 queries turned that into 17,549 self-intersecting faces against a true 861. | glTF 2.0 Skins, `check/gltf_mesh.rs` |
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

```rust
// mesh.holes: WELDED geometry, coincident verts joined at 1e-5, WORLD space.
// The metric's own negative: the same file with the weld step skipped.
assert_eq!(boundary_edges(&Surface::from_slice(&bytes)?), 171);
assert_eq!(boundary_edges(&Surface::unwelded(&bytes)?), 13_368);
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

Both rules read one rotation, split once. **T6 corrected the second formula**,
which as first written reads 108.805 degrees on a correct clip. See correction
1 below.

```
relative(b,t) = world(src,b,t)^-1 @ world(out,b,t)   # the source bone's frame
swing, twist  = split(relative(b,t), +Y)             # one split, per fact 19

clip.swing(b)  = max over t of  angle(swing)                            # ~0
clip.twist(b)  = max over t of |angle(twist at t) - angle(twist at rest)|  # ~0
```

- **`clip.swing`** is absolute against the vendor file. It reads about 0 on
  the legs, because both rigs' bones point the same way, and it is the rule
  that sees fact 4's unmeasured wrist.
- **`clip.twist`** compares each rig's twist against **its own rest twist**,
  read from the vendor FBX for the source and from `humanoid.glb` for ours.
  Still external truth, and it catches a re-rolled thigh while ignoring the
  174 degrees of convention difference.
- **Every mapped bone, terminals included.** Both rules report one finding per
  role in the convention table, hands and toes among them, so a bone that is
  out carries a number with its own name on it.
- **Frames align by seconds from clip start**, not by index, because
  requirement 9 removes a key so the ranges can differ by one.
- **Both limits are set by T6**, on a synthetic cross-rig fixture rather than
  on a committed CMU clip. See correction 2 below. `clip.swing` is 0.01
  degrees and `clip.twist` is 15.0.

**Pros:** sees the wrist, the 9.9 degree spine and a re-rolled thigh without
failing on the convention difference. Matches published practice, which
compares global joint state normalized by character height
(`research_art_pipeline_qa_systems.md:135`). A failure names the bone.

**Cons:** two rules and two limits, and a cross-rig calibration needs a second
rig, which T6 builds rather than downloads.

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
| 10 | The bone map is read from `humanoid.toml` only, with `retarget_chain`, `optional_roles` and `fingerprints` added | the prototype hardcodes a second copy |

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
never enters the swing math, so the rig's arbitrary rolls stop breaking
motion. No new dependency in a headless build, `humanoid.toml` stays the one
place bones are mapped, and the math is a pure function, which is what makes
rule four's CI tests possible.

**Cons:** we own the math, mitigated by the known-answer, negative-control
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

[aim_table]                    # world direction per role, Blender Z-up
hips = [ 0.0, 0.0, 1.0 ]       # facing -Y, so +X is his left
left_arm = [ 1.0, 0.0, -1.0 ]  # a direction, so the reader normalizes it
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
| `[retarget_chain]` | retargeting hierarchy, by role, decoupled from real parenting | a 3 spine source driving a 4 spine target |
| `optional_roles` | a role a source convention may leave out | a source with no shoulder or no toes |
| `[fingerprints]` | a bone that must exist for a convention to match | the **next** skeleton: after the rename our two tables are identical |

**`parents` means bones, and only bones.** T2 had already taken that word for
`[profile.parents]`, our own rig's real hierarchy, so T4 ships the retargeting
one as `[retarget_chain]`. It is keyed by role rather than by bone, and it is
the chain the transfer walks, so neither word describes the other's table.

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
            -> blender fixer -> clean.glb -> 11 of those 13 gates again
            -> POST /rigging { model_url: "data:model/gltf-binary;base64,..." }
            -> rigged.glb -> rig gates -> rename -> characters/<char>/model.glb
```

About 60 lines of plain `bmesh`. No addon, no credits, 55 ms. On `model.glb`
it took islands 8 to 1, holes 171 to 27, non-manifold 8 to 13, mirror error
3.02 to 0.00, and kept the texture and UVs. **T10 built the fixer and
measured every step of it: three of those four figures are different and one
defect class goes the wrong way**, see correction 1 below.

**Those numbers were provisional, and T12 replaced them.** They were measured
on `model.glb`, the rigged file, while the gate and the fixer run on
`bare.glb`. T3 published the `model.glb` figures into `[profile.mesh]` with
their headroom and marked every one for recalibration; T10 could not download
`bare.glb` either, so it built the fixer against a stand-in lifted out of
`model.glb`. **T12 regenerated the bare mesh** with the same request the
survivor was built with, because the paid task's own id answers 404 under the
key that exists, and every row is now read off
`art/staging/survivor/spike/unset/`. The readings are in correction 3 of T12.

**The two flags are independent.** `cleanup` alone decides whether the Blender
fixer runs at all, and therefore whether the two post-cleanup rules measure or
report `skipped` on that declaration; the pre-cleanup set applies to the mesh
as it arrived either way. `symmetry` alone decides whether symmetrize is part
of that fixer and whether the three mirror rules measure or report `skipped`
on **its** declaration, which is the word T2 added beside `info` for exactly
this. Flipping either is a reviewed diff in `spec.ron`, and the negative
control for each silenced rule still runs in CI, so the proof that the rule
works never leaves with it.

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
is reviewed for both. Vertex count moves, 27,508 to 28,219 on the stand-in, so
weights are recomputed, which is why the re-rig is in the same step. Whether
Meshy returns a symmetric skeleton from a symmetric mesh is the bet, and it
costs 5 credits to settle.

**T10 measured four more, on the stand-in:** holes 40 to 73, pieces 1 to 2,
crossing faces 905 to 1026, and the height 0.349 percent under the 1.700 m the
spec asks for, because the half the mirror keeps is the shorter one. So the
trade is one sentence: symmetrize removes all 3.021 percent of the asymmetry
and makes three topology counts worse, one of them past its published ceiling.
This decision was accepted on the condition that it "makes things better
without worsening things we care about", and on the stand-in that does not
hold for `mesh.self_intersect`. The real `bare.glb` is what decides:
correction 1 of T10 has every reading and what a human does about the
crossings row, and correction 13 has the three alternatives that were measured
and refused.

**Rationale:** Accepted, on for the survivor.

#### ❌ Option 2: Meshy's paid AI Auto-Repair

```jsonc
{ "model_url": "https://..." }   // POST /openapi/v1/print/repair, 10 credits
```

**Pros:** automatable, and it names holes and watertightness in its fix list.

**Cons:** it strips textures and rigging then refuses the mesh (fact 11), and
it makes no claim to fix self-intersections or merge islands.

**Rationale:** Rejected, decision 5. It fails the "no worsening" test twice.

#### ❌ Spike, measured: which `pose_mode` to send. None of them

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

**Named acceptance test, `spike_pose_mode`, run in T12 as `cargo art
spike-pose survivor --rig`.** Regenerate the model stage three
times from the same four committed concept views: unset, `"a-pose"`,
`"t-pose"`. Run every `mesh.*` and `rig.*` gate on each. `"a-pose"` is adopted
only if all four hold.

1. Humerus within 15 degrees of the 40 the prompt asks for.
2. Elbow bend at rest under 5 degrees.
3. Welded `mesh.holes` on `bare.glb` no worse than T10's recalibration.
4. A human confirms on the model contact sheet that no forearm surface was
   invented.

If `"a-pose"` fails and `"t-pose"` passes 1 to 3 but fails 4, the field stays
unset and the measurement is recorded. Cost 0.5 days plus about 90 credits at
0.013 USD each, so about 1.20 USD.

**Rationale: the field stays unset**, and `Subject::pose_mode` exists so the
measurement can be re-run rather than re-argued. T12 spent 105 credits on it.
The API does know the field and validates it by name, so step 0 answered yes,
and then both values it accepts made the mesh far worse: `a-pose` returns
**1288** boundary edges and `t-pose` **999**, against the unset request's
**31**. `a-pose` is the only mode whose humerus lands inside its band, and it
also drops the sideways `Hips` of fact 1 from 95.7 degrees to 5.8, which is
the best rig this project has measured. It is still not adopted, because
adoption needs all four items and item 3 fails by 6x. No mode passes item 2:
every rig Meshy sells bends at the elbow at rest, 18 to 28 degrees. Every
reading, and what the three contact sheets show, is in the T12 corrections.

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
    concept(&spec, &paths)                     # four fresh views, always
    report = check(Stage::Concept, attempt)    # every concept.* rule
    print what it measured
    if no error: break
    if attempt == 3: bail with all three reports, images kept on disk
    attempt += 1
```

- **Three generations in total**, one plus two regenerations, at four OpenAI
  images each. About 0.80 USD per attempt and 2.40 USD for all three.
- **The stage reuses nothing, attempt 1 included.** Correction 13 has the
  detail: a set on disk is either one a previous run left failing, which no
  retry budget should be spent re-measuring, or one this run was told to
  replace. It always regenerates **all four views**, because a new front view
  invalidates the three derived ones.
- **The wrapped stage is not free.** `Stage::Concept` bills OpenAI (fact 14),
  so `ConfirmSpend` is asked once, quoting 2.40 USD for up to three attempts.
  The invariant is that the loop **never wraps a Meshy stage**: a failing
  `mesh.*` or `rig.*` gate stops the run and reports.
- Each attempt writes `art/staging/reports/concept.<char>.<attempt>.json` and
  every Finding carries an `attempt` field, so all three attempts survive.
  Each also prints its own summary line, the same one `cargo art check`
  prints, so a failing attempt names its report on the terminal.
- The `concept.*` limits are calibrated in the same task that adds the loop,
  so no uncalibrated threshold can ever spend money.

**Two mesh gates, because they cost nothing and see different things.**
`print/analyze` is free, needs no Blender and runs on the bare mesh before
rigging, by the model task's own id, which is what `clean.glb` does not have.
Its `non_manifold_edges` counts boundary and true non-manifold edges together,
which is why it reads 179 on `model.glb` where we read 171 plus 8, and that
goes in `measured_on`. Our own Rust pass measures the rest on welded,
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
few frames. Those numbers go into `art/staging/reports/fetch.<clip>.1.json` as
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
the judgment half. No new command.

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
math moves into `transfer.py`, which never imports `bpy`. The `model` stage
gains a download and a local cleanup, so rigging is fed a cleaned mesh.

```text
spec.ron
  ▼ concept  ─▶ [concept.*  background, one figure, arm gaps, mirror, cross_view]
               fail ─▶ regenerate with force, 3 attempts total, 2.40 USD
  ▼ model    ─▶ download bare.glb
               [mesh.printability  print/analyze, 0 credits, no Blender]
               [mesh.*  WORLD space then WELDED: holes, non_manifold, islands,
                        self_intersect, mirror, world_size, facing, budget,
                        uv, texture, quads, stray_object]
               blender fixer (opt-in)  ─▶ clean.glb
               [mesh.cleanup_effective  lt]  [mesh.non_manifold_post  le]
  ▼ rig      ─▶ POST /rigging { model_url: data URI }   (5 credits)
               [rig.*  bone_set, parents, single_root, child_axis, mirror,
                       world_height, humerus_angle, facing, object_transform,
                       bind_deviation, up_axis, names_standard]  ─▶ rename
               [gltf.validator]
  ▼ fetch    ─▶ [source.*  posture, child_axis, wander, fps_declared,
                          traveling / in_place]              (library.lock)
  ▼ retarget ─▶ [clip.fps_grid] [clip.fps_grid.range] [clip.loop]
               [clip.object_transform] [clip.interpolation]
               [clip.swing  absolute vs the vendor file, every mapped bone]
               [clip.twist  change from each rig's own rest twist]
               [clip.foot_contact.*] [clip.floor_snap]
               [clip.stride  against the source's travel] [clip.stride_ratio]
  ▼ bake     ─▶ [clip.root_travel, clip.root_bob  on the stripped copy,
                                                  max over frames]
               [bake.*  frame_count, non_empty, in_frame, pivot, forearm_roll,
                        sampled_frames_are_keys, landmark_golden]
  ▼ pack     ─▶ [atlas.*  frame_count, trim_boxes, manifest_schema]
               sheet.png, full under art/preview/ and downscaled beside the atlas
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
| Self-intersection counting | **`parry3d` 0.30.2, its `Bvh`** | hand-rolled BVH, `pymeshlab` | dimforge, maintained, ships the broad-phase `Bvh` plus triangle queries. `Qbvh` was removed and replaced by `Bvh`. This is the one measurement `bmesh` never did (`research_concept_and_model_stage.md:40`). Default features off: `alloc`, `required-features` and `std` only, which drops `spade`, the Delaunay triangulation no gate does |
| glTF structural validation | **npm `gltf-validator` under Bun** | Homebrew, a precompiled binary, a Rust wrapper | there is no Homebrew formula and the GitHub binaries are x64. The npm package is Dart compiled to JS, so it is architecture independent (fact 16), and `validateBytes()` returns a report with severities |
| Concept image checks | **`opencv-python-headless`** | Pillow plus numpy, rembg | the arm-gap check needs contour hierarchy (`findContours` with `RETR_CCOMP`) |
| Blender in CI | **Not used** | container, an x86_64 runner | fact 16 |
| Art sign-off | **Pull request approval** | Chromatic, Skia Gold, a lock field | decision 13. GitHub already forbids self-approval |

## Structure

```text
art/
  skeletons/humanoid.toml   # roles + [retarget_chain] / optional_roles
                            #       + [fingerprints] + stride_segment
                            #       + NEW ground_roles  the joints on the floor
                            #       + NEW [profile]     every published limit
                            #       + NEW [aim_table]   world aim per role
  animations/library.ron    # + NEW source_fps and travels per animation
  animations/library.lock   # + NEW verdict on Fetched
  goldens/survivor/         # NEW  3 frames x 2 directions per clip, text
                            #      24 joints a frame, 73 lines a file
  staging/                  # gitignored: bare.glb, clean.glb, reports/
  preview/                  # gitignored: full size local scratch

project/assets/characters/survivor/
  sheet.png                 # NEW  committed contact sheet, LFS, by the atlas

crates/xtask-art/src/check/
  mod.rs                    # Finding, Severity, comparison, attempt, runner
  gltf_world.rs             # world transforms from the glTF node graph
  rig.rs                    # rig and object conformance, from [profile]
  aim.rs                    # NEW  [aim_table], and rig.aim_table
  gltf_mesh.rs              # NEW  the surface: world, then weld
  mesh.rs                   # NEW  the mesh rules, from [profile]
  clip.rs                   # NEW  swing, twist, fps grid, object transform
  gltf_clip.rs              # NEW  the delivered clip, sampled at its key times
  motion.rs                 # NEW  one clip's orientations, from either reader
  bake.rs                   # NEW  the rendered frames, and the spec patch
  atlas.rs                  # NEW  pack and manifest invariants
  validator.rs              # NEW  runs npm gltf-validator, maps its report
crates/xtask-art/src/
  spec.rs                   # + Subject::cleanup, Subject::symmetry
  lock.rs                   # + content hashes, + Model in the version guard
  library.rs                # + Animation::source_fps, travels, + verdict, + fingerprint
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
  check/concept.rs          # NEW  the five image checks, in Rust (Rule 4)

tools/gltf_validator/
  validate.mjs              # NEW  drives the npm validator, which has no CLI

tools/blender/src/
  skeleton.py               # NEW  the skeleton file: roles, chain, aim table
  transfer.py               # NEW  pure math: matrices in and out, no bpy
  clip.py                   # NEW  pure counts, and the source motion sidecar
  plant.py                  # NEW  pure math: contact detection, 2 bone IK
  findings.py               # NEW  the shared Finding record and JSON writer
  retarget_animation.py     # bpy glue only: import, map, transfer, export
  cleanup.py                # NEW  the fixer's order and decisions, no bpy
  mesh_clean.py             # NEW  the fixer only. It measures nothing
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
  than trusting an importer, and the same specification explains fact 8. **The
  node transform of a skinned mesh must be ignored**, so `check/gltf_mesh.rs`
  places a skinned vertex by `joint world * inverse bind matrix` and an
  unskinned one by the node chain (fact 20). glTF has no `quads` primitive
  mode, so the spec's `quads: true` cannot be verified from a delivered GLB;
  what `mesh.quads` reads instead is whether every primitive is triangles,
  because a primitive of points or lines is one no other mesh rule measured.
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
tables. `check/rig.rs` reads `[profile]`, `transfer.py` and `check/aim.rs`
both read `[aim_table]`,
and **every published limit in this design lives in `[profile]`**, including
the `clip.*` and `bake.*` ones. Adding a skeleton means adding a file.

**The aim table is validated, not just parsed**, because it is the single
source of every bone's constant offset and a sign error on one row produces a
confidently wrong clip. Three rules: every role in the convention table has a
row, and a missing row is an error rather than a fallback to the source's rest
pose. Mirror rows are exact reflections across the mirror plane. Each
prescribed aim sits within `max_bind_deviation_degrees` of **both** rigs' own
rest aim, so a table that describes neither rig cannot pass.

**The first two are load-time refusals and the third is a gate**, because it
needs a rig in hand. Both readers refuse the table they read: `skeleton.py`
for the transfer and `check/aim.rs` for the gate. The third is
`rig.aim_table`, in Rust, per rule four: it reads a rig's rest aim off the
glTF node graph, which `check/gltf_world.rs` already owns, and it reports one
finding per role. **A rig's own rest aim is its bone's own `child_axis` in
world space**, which is the same direction the table prescribes and not the
joint-to-child direction `rig.child_axis` measures. "Both rigs" is therefore
one rule run twice: on ours at the rig stage, and on a source rig when its
motion arrives.

**Every gate returns the same record.** Rust emits it, Python emits it from
inside Blender, and Rust parses it.

```jsonc
{
  "rule": "clip.swing",             // stable id, printed by --list-rules
  "severity": "error",              // error | warning | info | skipped
  "subject": "left_hand",           // role, bone, file, frame, object or direction
  "measured": 52.9, "limit": 0.01,
  "comparison": "le",              // le | lt | eq | ge  -- REQUIRED
  "unit": "degrees",
  "attempt": 1,                    // which regeneration produced this
  // The rule's own registered space, verbatim. `Report::disagreement`
  // refuses a finding that words it any other way.
  "measured_on": "Blender Z-up world space, the output bone against the source bone, aligned by seconds from clip start, worst frame of the clip",
  "message": "LeftHand points 52.900000 degrees from the source's left_hand at 0.233 s"
}
```

`comparison` is required and load-bearing. Without it, `cleanup_effective`
with `measured = after, limit = before` passes a fixer that changed nothing,
at 8 le 8.

Contract for callers:

- Exit code is non-zero if and only if at least one `error` is present.
- **The comparison decides the severity, never the caller.** A rule reports
  every subject it resolves: `error` when the comparison fails, `info` with
  the number when it holds, `skipped` when a declaration switched the rule off,
  `warning` when a remote call was unavailable. Findings are built through the
  `Rule` registry `--list-rules` prints, so a rule cannot report a limit, a
  unit or a space that the printed list does not carry. The one exception is
  an undefined measurement, which carries its own unit and no limit because
  this rule's would be a lie; the registry recognizes that shape and still
  holds it to this rule's space.
- A missing input is an error, never a skip. A missing golden is an error. An
  unavailable remote call is a `warning`, never silence.
- `measured_on` is mandatory and names the space. A rule with no space fails
  its own unit test.
- **A gate never emits NaN.** Where a measurement is undefined, it reports the
  reason as an `error` with a stated message. `clip.twist` at 180 degrees of
  swing is the one case, and its message is
  `"swing is 180.000 degrees, twist undefined"`. **T6 made that a band rather
  than a point**, at 179.9 degrees and past: the file stores `f32`, so nearer
  than that the twist has fewer digits left than the tightest limit any gate
  publishes. A swing that far out is already an error of its own.
- The runner writes `art/staging/reports/<stage>.<item>.<attempt>.json` on
  every run, one stem per clip or character so nothing overwrites a sibling,
  so no retry overwrites the attempt before it.

**Published limits.** Every value lives in `[profile]`. Calibration assets and
measured values are in the Test Plan, once, so the two cannot drift.

| Rule | Limit | Comparison |
|---|---|---|
| `concept.background_flat` | 12 levels of 0 to 255, from the 3, 6, 3 and 6 the four committed views read, so 2x the worst of them. Read over the region the border fill reaches and no further, which the finding's message states as a share. The reading saturates near twice the 12 levels the silhouette is classified at, so it refuses any ramp wider than about 12 levels: a 30 percent one reads 24 | le |
| `concept.single_figure` | 1 figure. A count has no tunable limit, so this is the second family with no `[profile]` number. What is calibrated is the speck floor, 0.05 percent of the image: the committed figures run 204,657 to 371,283 pixels and no other piece is over 4 | eq |
| `concept.arm_gap` | 75 percent of the torso band's rows, from the 86.054 and 86.316 the front and back views read, so 11 points under the worst of them. The 14 percent that do not show a gap are the shoulder rows at the top of the band | **ge** |
| `concept.mirror` | 2.4 percent of the width, at the 99th percentile of the rows, from the 1.167 and 1.062 the front and back views read, so 2x the worst of them. Read on each row's leftmost and rightmost figure pixel, so interior asymmetry is invisible to it. Neither the mean nor the outright worst row: correction 4 has all three readings | le |
| `concept.cross_view` | 6.0 percent of the mean of two heights, from the worst of the six pairs, front against back at 3.256, so 1.8x over it | le |
| `clip.swing` | 0.01 deg, set by T6 on a synthetic cross-rig fixture | le |
| `clip.twist` | 15.0 deg, the same fixture plus the A-pose against T-pose residual | le |
| `clip.root_travel` | 0.02 m, on the two horizontal axes the strip pins | le |
| `clip.root_bob` | 0.15 m on the up axis, which the strip keeps. Calibrated on the four fitted clips at 0.0089 to 0.0535 m, so it sits 2.8x over the worst of them and still refuses the 0.2911 m the old strip sank a left strafe by | le |
| `clip.floor_snap` | 5 mm from the rest height the snap aims at. Calibrated at both sites, per the Test Plan row: worst reading 6.7e-7 m, so it sits 7,493x over that and still refuses the 0.0599 m the same fit leaves with the lift removed | le |
| `clip.stride` | 2.0 percent of the source's own travel sized by the femur ratio. Worst reading 2.4e-5 percent across both sites, so it sits 81,865x over that, and a fit sized five percent out reads 5.0 | le |
| `clip.stride_ratio` | 100, further apart than two rigs of one skeleton can be, so every reading is `info`. Records rather than gates: the readings are in the Test Plan row | le |
| `clip.foot_contact.plants` | 1 per foot per cycle. `travels: false` switches it off, because an in-place cycle's ground moves under it and its feet slide by construction: `run.glb` slides them at 3 to 5 m/s | ge |
| `clip.foot_contact.skate` | 2.5 cm at 180 cm scale. With the lock in, the three Mixamo fits read exactly 0.0000 m; with it removed the same fits read 0.0207, 0.0148 and 0.0027 | le |
| `clip.foot_contact.penetration` | 5 mm from the ground plane at zero. The vendor's own `strafe_left.fbx` reads 0.0000 m at every planted frame, and a refit of `run.glb` onto its own rig clears the floor by 0.0005 m. The three cross-rig fits read 0.0200 to 0.0203 m, which is correction 4 of T9 | le |
| `clip.fps_grid` | 1e-4 frames | le |
| `clip.fps_grid.range` | 0 frames between what the retarget samples and what the source keyed | eq |
| `clip.loop` | 2.0 deg, today's `LOOP_TOLERANCE_DEG` | le |
| `source.fps_declared` | 0 frames per second between the file's rate and the library's | eq |
| `source.traveling`, `source.in_place` | 0.02 m of hip travel, the threshold both ways. Two ids because one comparison per id is what the registry holds a report to: `travels` picks which half measures and the other reports `skipped` | ge, le |
| `source.wander` | 1000 m, further than any clip moves its hips, so every reading is `info`. Records rather than gates: an in-place cycle wanders 0.0276 m and a strafe 2.3117, and no one threshold reads both | le |
| `source.child_axis`, `source.posture` | 180 deg, the largest angle two directions can be apart, so every reading is `info`. Both record rather than gate | le |
| `rig.child_axis` | 2.0 deg | le |
| `rig.mirror_length`, `rig.mirror_direction` | 1.0 percent, 1.0 deg | le |
| `rig.humerus_angle` | 15 deg from the target of 40 | le |
| `rig.elbow_bend` | 180 deg, the largest angle two directions can be apart, so every reading is `info`. Records rather than gates: every rig Meshy has sold this project bends at rest, 24 degrees on the committed one, and a limit that failed it would fail on every run until that rig is regenerated | le |
| `rig.world_height` | 5 percent of `spec.subject.height_meters` | le |
| `rig.bind_deviation`, `rig.aim_table` | 75 deg | le |
| `rig.names_standard`, `bone_set`, `single_root`, `parents`, `facing`, `up_axis`, `object_transform` | 0 defective bones, and exactly 1 bone per declared name for `bone_set`. A count of defects has no tunable limit, so these are the one family whose limit is not a `[profile]` number | eq |
| `mesh.non_manifold_post` | 20 edges. T10 read 12 on a stand-in's `clean.glb` and T12 reads **17** on the real one | le |
| `mesh.cleanup_effective` | the pre-fixer count, over holes and islands. Self-intersections left the set in T10, correction 1, because mirroring copies them | **lt** |
| `bake.in_frame` | 1 px of alpha inset. The tightest of the 848 rendered frames is 58 px clear of a border, and a pose the camera cut off reads 0 | ge |
| `bake.pivot` | 2 px between two opposite directions and their own reflection about the canvas center. **Not ground-line drift**, which reads 28 to 86 px on correct art: correction 1 | le |
| `bake.non_empty` | 1.0 percent of a frame's own canvas. The emptiest frame of each clip reads 4.1164, 4.8367 and 4.8912, and a frame that rendered nothing reads 0.0000 | ge |
| `bake.landmark_golden` | 1 px, which is one rounding step of a golden's own whole pixels. All 432 committed landmarks read 0 | le |
| `bake.forearm_roll` | 0.0 | eq |
| `bake.sampled_frames_are_keys` | 0 rendered frames that are not authored keys | eq |
| `bake.frame_count`, `atlas.frame_count`, `atlas.trim_boxes`, `atlas.manifest_schema` | 0 defects each: a frame absent from the rendered rectangle, a cell with no rect, a box outside the atlas or its cell, a manifest the game's own reader refuses. These join the `rig.*` family whose limit is not a `[profile]` number | eq |

**The `mesh.*` limits, set by T3 and recalibrated by T12.** T3 read them off
`art/characters/survivor/model.glb`, the file **after** rigging, because
`bare.glb` could not be downloaded (fact 10), and marked every one
provisional. T12 regenerated the bare mesh with the same request the survivor
was built with and set them from it:
`art/staging/survivor/spike/unset/{bare,clean}.glb`, welded at 1e-5 m in world
space, reported in `art/staging/reports/{mesh,cleaned,cleanup}.survivor-unset.1.json`.

| Rule | Measured on the real bare mesh | Limit | Headroom | Comparison |
|---|---|---|---|---|
| `mesh.holes` | 31 boundary edges, 18 after the fixer. `model.glb` read 171 | 200 | 169, 84 percent | le |
| `mesh.non_manifold` | 6 edges. `model.glb` read 8 | 10 | 4, 40 percent | le |
| `mesh.islands` | 4 pieces, 1 after the fixer. `model.glb` read 7 | 8 | 4, 50 percent | le |
| `mesh.self_intersect` | 1094 faces, and 1153 after the mirror copies half of them | 1,500 | 347, 23 percent | le |
| `mesh.mirror` | 3.257 percent of width, 0.000 after the fixer. `model.glb` read 3.021 | 3.5 | 0.243, 7 percent | le |
| `mesh.world_size` | 1.8970 m against a spec of 1.700, 11.588 percent | `mesh.height_percent`, 25 percent | 13.4 points, 2.1x | le |
| `mesh.budget` | 54,909 triangles, 55,553 after the fixer | 300,000, Meshy's stated rigging limit | 244,447, 81 percent | le |
| `mesh.printability` | not measurable on a file. The design records 179 | 200 | 21, 12 percent | le |
| `mesh.non_manifold_post` | 17 edges on the real `clean.glb`. The stand-in read 12 | 20 | 3, 18 percent | le |
| `mesh.facing`, `stray_object`, `uv`, `texture`, `quads` | 0 defects each | 0. A count of defects has no tunable limit, so these join the `rig.*` family whose limit is not a `[profile]` number | none, by design | eq |

`mesh.world_size` no longer reads the rig's 5 percent band, and correction 3
of T12 has why: `height_meters` is a parameter of the **rigging** call, so
nothing before it scales the body.

**The `[profile.cleanup]` table is not limits.** It is the two sizes the fixer
acts at, published so no script holds a copy: `smallest_island_cubic_meters`
at 1e-6, which is a 1 cm cube measured exactly in `test_cleanup.py`, and
`symmetrize_meters` at 0.001. The weld distance is not there, because
`check/gltf_mesh.rs` already owns it and two copies of one distance drift.

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
- Role-based matching itself, which `[profile]` extends in place. **T4 moved
  the reader**: `framing.SkeletonRoles` is now `skeleton.Skeleton`, in the
  module that owns the whole skeleton file, and `framing.py` keeps the bake's
  own geometry. The role tables did not change.
- `framing.py::sampled_frames`, which already rounds to integers (fact 6).
- The `sys.exit(1)` wrappers on every Blender script.
- `scale_translation` and `translation_scale` as operations. Only the metric
  feeding them changes, from total height to femur length.
- `ConfirmSpend`. It guards money and stays the one mid-run touchpoint.

**Extended:**

- `spec.rs` gains `cleanup` and `symmetry` on `Subject`.
- `library.rs` gains `Animation::source_fps`, `Animation::travels`,
  `Fetched::verdict` and `Fetched::fingerprint`.
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
| `loop_mismatch`, `report_loop` | `report_loop` warns and never refuses. `clip.loop` replaces it with a limit and a comparison. **T5 deleted both already**, with the 14 tests its correction 13 lists, so T7 found nothing left to delete |
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
    rule, limit = ("clip.root_bob", 0.15) if axis == "z" else \
                  ("clip.root_travel", 0.02)
    finding(rule, measured=value, limit=limit, comparison="le",
            subject=f"{clip} {axis}",
            measured_on="world space, after strip_root_motion")

ratio = femur_length(ours) / femur_length(theirs)   # not total height
for key in every_location_key(action):              # the same operation
    key.co[1] *= ratio
# The floor is where our own rest pose stands, not zero: the toe joint is the
# ball of the foot and rests 0.0307 m above the sole.
floor = min(world_z(toe, rest) for toe in GROUND)
lift = floor - min(world_z(toe, f) for toe in GROUND for f in frames)
offset_root_by(lift)
finding("clip.floor_snap", measured=abs(lowest_after - floor), limit=0.005,
        comparison="le", subject=f"{toe} at frame {f}")
finding("clip.stride", measured=percent(travel(root), travel(hips) * ratio),
        limit=2.0, comparison="le", subject=clip)
```

`clip.stride` holds the fit to the source it was bought from: the root travels
first frame to last, in world space and before any strip, against the source's
own hips sized by the same femur ratio. A clip the library declares in place
has no travel to be sized, so `travels` reports it `skipped`.

All three findings are taken again in Rust on the delivered GLB, per rule
four. The sidecar `retarget_animation.py` already writes for `clip.swing`
therefore carries two lengths beside its rotations, `travel` and
`stride_segment`, because the vendor file is an FBX no Rust reader opens. Our
own femur is **not** written there: the Rust half measures it off the rig
GLB's rest joints, so the two sides of the ratio have two readers. Correction
7 records what that catches and what it cannot.

Both run after `strip_root_motion`, which pins the two horizontal axes in
world space (fact 5), so `clip.root_travel` can only ever read a residual and
`clip.root_bob` reads the bob the strip keeps. Neither is a substitute for
`source.traveling`, which is why that rule is symmetric, nor for
`source.wander`, which is the excursion the pin removes.

**Foot planting** (requirement 6). Thresholds are world-space meters scaled
from the published 180 cm reference, and the speed threshold is a rate, so it
means the same thing at 8 fps and 30 fps. What they are read on is the
**sole point** under a joint, never the joint itself: correction 1 of T9 has
why, and correction 2 has how the sole is derived on a clip with no mesh.

```python
scale = rig_joint_span_m / 1.80
speed = lambda f: step_xy(ball, f) * source_fps       # meters per second
contact = [f for f in frames
           if world_z(ball, f) < 0.03 * scale and speed(f) < 0.30 * scale]
width = max(3, round(5 * source_fps / 60) | 1)   # odd and >= 3, or no majority
contact = majority_vote(contact, width)
runs = consecutive(contact)
finding("clip.foot_contact.plants", measured=len(runs), limit=1,
        comparison="ge")
for run in runs:
    lock_xy(ball, run, ramp_in=2, ramp_out=2)         # two bone analytic IK
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
through the glTF node chain and then welded, and reports the pair. Both are
unskinned, which is why the node chain is the whole transform there and why a
skinned file needs fact 20's path instead.

**T10 built this and corrected two lines of it.** Every step is a `bmesh`
operation, the mirror included, so the last line is
`bmesh.ops.symmetrize(direction="X", dist=...)`, which is the same half and
the same threshold under the spelling that API accepts. Every constant
arrives on argv from `[profile.cleanup]` and from the weld distance
`check/gltf_mesh.rs` owns, so none of the four numbers above is written in
Python at all. Corrections 5 and 10 have both measurements.

## Edge Cases & Constraints

- **A gate that measures the wrong representation gives precise, wrong
  numbers.** Seam splitting turned 171 holes into 13,368, so every rule states
  its space and is calibrated before its limit is trusted.
- **A normalized axis is not a vector part.** Fact 19 is the same class of
  fault one level down: the wrong projection is invisible on a large twist and
  worst near identity, which is why the split ships with two numeric
  known-answer tests rather than one comparison.
- **The weld distance is itself a parameter.** Too large and it merges
  distinct vertices, hiding holes. **T3 measured the histogram on `model.glb`
  in world space and 1e-5 m sits inside the plateau**, so the assumed value
  turned out to be right and is now evidence rather than a guess:

  | Weld distance | Welded vertices | Boundary edges | Non-manifold edges | Islands |
  |---|---|---|---|---|
  | none | 34,490 | 13,368 | 0 | 413 |
  | 1e-9 m to 1e-4 m | 27,508 | 171 | 8 | 7 |
  | 1e-3 m | 27,488 | 164 | 45 | 7 |
  | 1e-2 m | 9,885 | 5 | 16,210 | 5 |

  Every cell is asserted in `test_gltf_mesh.rs`.

  The two ends are the two failure shapes. Skip the weld and 171 holes read
  as 13,368 and 7 islands read as 413, while every seam hides the 8 real
  non-manifold edges and the count reads 0. Weld a hundred times too wide and
  166 of the 171 holes vanish while the non-manifold count goes from 8 to
  16,210, invented out of merged geometry. The plateau runs four orders below
  1e-5 m and four above it, which is why that distance is safe rather than
  lucky. T3 added the 1e-9 row to the four the design named, because the
  lower edge is what proves 1e-5 is not sitting next to a cliff.
- **Two tools agreeing is not evidence.** Our `bmesh` scan and Meshy's
  extension matched to the integer and both were wrong, sharing the input.
- **Named blind spot: joint-chain posture on a rig that is not conformant.**
  Every gate this design publishes for a clip measures a bone's own frame.
  `clip.swing` is the quantity the transfer drives to zero **by
  construction**, because aiming both rigs at one table makes every offset a
  pure twist about the bone's own axis, so it reads about 0 on a correct fit
  and cannot be evidence of anything else. What it **can** catch is everything
  between the math and the file: a Blender shell that wrote something other
  than what `transfer.py` computed, a role driving the wrong bone, a dropped
  or duplicated frame, a key at the wrong time, and an export that lost or
  resampled the motion. Read 0.000 as "the file carries the motion the
  transfer computed", never as "the clip looks right". `clip.twist` compares
  each rig against its own rest, so a difference that is in the rest is
  exactly what it subtracts out. `bake.landmark_golden` is a golden against
  our own output, so it pins a regression and never an error. Nothing in that set
  sees a **joint** chain: with `rig.child_axis` failing on `Hips` by 97.6
  degrees, the survivor's spine lean comes out 22 degrees from the source's
  and every gate stays quiet. T5 found it by hand, from four angles measured
  on the vendor file, and the pipeline would not have. Mitigation, in two
  parts, because the fault has two owners: **T15** regenerates our rig so its
  own axes point at its own children, and **`source.child_axis`** in T7
  reports the vendor's half at fetch time, since that skeleton is not ours to
  fix. Until T15 the number is a known, measured, recorded difference rather
  than a silent one. T9's correction 3 is the **position** half of this same
  blind spot, found by the first rule that reads a point rather than a bone's
  own frame: the 97.6 degree hips lift one hip socket 0.1956 m and the right
  foot never lands, and every rule in the set above stays quiet about it.
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
  `clip.object_transform` is defined as "every node beside the joints carries
  the transform the committed rig gives it, and nothing animates one", not as
  a magnitude test. **T6 corrected both halves of the old definition**, which
  were "every object matrix is identity" and "rest bone matrices byte-match":
  neither is true of any file this pipeline has ever written. See correction 4
  below.
- **A `YZX` euler cannot separate twist near a Z component of 90 degrees**, and
  our `Hips` sits there. The quaternion split has one singularity, at 180
  degrees of swing, and it is handled rather than argued away: `transfer.py`
  raises `swing_singular`, and `check/clip.rs` reports an error from 179.9
  degrees on, because 180 degrees of swing from rest is reachable on animated
  data.
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
tests the metric rather than the gate. Every `concept.*`, `mesh.*`, `rig.*`,
`clip.*`, `bake.*` and `atlas.*` row runs in Rust, so its negative control is
a required CI check with no Blender. **`source.*` cannot**: the vendor file is
an FBX no Rust reader opens, so those six are measured in Python and their
negatives are pytest ones, on the math module `source.py`, with a report from
a real run committed as `crates/xtask-art/tests/fixtures/fetch.run.1.json`.
`source.posture`, `source.child_axis` and `source.wander` are recording rules,
so they are calibrated and carry no negative, per the Terminology exemption.
**Two `bake.*` rows cannot either, and T14 says which:**
`bake.sampled_frames_are_keys` reads the clip's own action and
`bake.landmark_golden` projects through the scene camera, so both are measured
in `framing.py` and both negatives are pytest ones, which CI runs in the same
`pytest` job as `source.py`'s. The other five read PNGs and a spec field,
which need no Blender.
**`mesh.quads` was in that sentence and T3 took it out:** a rule that can
only report `info` is failure shape one from the section above, and the thing
it can honestly measure does fail. See the correction below.

| Rule | Positive | Negative fixture it must reject | Calibration |
|---|---|---|---|
| `concept.background_flat` | the four committed views, 3, 6, 3 and 6 levels against 12 | `[synth]` a 30 percent gradient added to `front.png`, 24 levels | the four views. Measured on the pixels a border flood fill reaches, per channel, 1st to 99th percentile |
| `concept.background_flat`, the share it read | `front.png`, 76.394 percent of the image filled | `[synth]` a second backdrop tone over the top-left fifth of `front.png`: still 3 levels, and 72.412 percent filled | not a limit. The share rides in the message, and `concept.single_figure` is what refuses the case, at 2 figures |
| `concept.single_figure` | the four committed views, 1 figure each | `[synth]` `front.png` twice at half size, side by side, 2 figures against 1 | the speck floor, 0.05 percent of the image or 786 pixels: the four figures run 204,657 to 371,283 pixels and no other piece is over 4 |
| `concept.arm_gap` | `front.png` 86.054 percent and `back.png` 86.316, against 75 | `[synth]` both gaps of `front.png` painted shut with skin, over rows 380 to 680, 0.000 percent | the two views. The band is 25 to 45 percent down the silhouette, which lands on rows 384 to 677 and 384 to 668, and a gap is a background run of at least 4 pixels: a drawn 3 pixel run reads 0.000 percent of rows and a 4 pixel one 100.000 |
| `concept.mirror` | `front.png` 1.167 percent and `back.png` 1.062, against 2.4 | `[synth]` `front.png` with the arm rows stretched outward 15 percent from the middle of the frame, 6.394 percent | the two views, at the 99th percentile of their rows. Correction 4 records the mean and the worst row of all three too |
| `concept.cross_view` | the six pairs of the four committed views, 0.757 to 3.256 percent against 6.0 | `[synth]` `left.png` at 92 percent of its size, centered on its own backdrop: 10.549 percent against the front and 7.299 against the back | the six pairs, on silhouette height and vertical centroid, as a percent of the mean of two heights |
| `mesh.printability` | a recorded response | `[synth]` a recorded response over the ceiling, `13368` edges and `is_watertight: false` | **not calibrated: the Meshy key answers 401, so no response could be recorded.** The ceiling of 200 comes from the design's own 179. `is_watertight` is Meshy's summary of the same count, so it goes in the message and is not measured twice |
| `mesh.holes` | `model.glb`, world then welded, 171 | `[synth]` 67 boxes each missing a face, 201 edges against 200, **plus** 66 of them at 198, which passes. `[mut]` the same file unwelded, 13,368 | T12, on the real bare mesh: 31 |
| `mesh.non_manifold` | `model.glb`, welded, 8 | `[synth]` 11 edges each carrying 3 faces, against a limit of 10 | T12, on the real bare mesh: 6. **This row was missing from the design**, see the correction below. A file the fixer wrote is not read against it: T10, correction 8 |
| `mesh.non_manifold_post` | the real `clean.glb`, 17 edges against a ceiling of 20 | `[synth]` 21 boxes each with 3 faces on one edge, against that ceiling, **plus** 20 of them, which passes | T12, on the file the fixer wrote from the real bare mesh. T10 read 12 on a stand-in |
| `mesh.quads` | `model.glb`, every primitive is triangles | `[synth]` a primitive declared as lines, which every other rule then reports as undefined | glTF has no quad mode, so what this rule reads is whether the surface is readable at all. **It was specified as `info`, never `error`**, see the correction below |
| `mesh.islands` | `model.glb`, welded, 7 | `[synth]` 9 separate boxes against a limit of 8. A 5 mm cube inside the mesh is the second piece and is asserted at 2 | T12, on the real bare mesh: 4, and 1 after the fixer |
| `mesh.self_intersect` | `model.glb`, welded, via `parry3d`, 861 | `[synth]` 120 interpenetrating box pairs, 1,680 faces against 1,500. One pair alone is asserted at 14, and a 5 mm cube **inside** the mesh at 0 | T12, on the real pair: 1094 arrive and 1153 survive the mirror. **The design's 16 could not be reproduced**, see the correction below |
| `mesh.mirror` | `model.glb`, 3.021 percent, inside its own pre-fixer limit | `[synth]` one side pushed out 2 cm on a 32 cm figure, 6.25 percent against 3.5 | T12, on the real bare mesh: 3.257. `model.glb`'s own 3.021 agrees with fact 9's independent kd-tree pass to three decimals |
| `mesh.world_size` | `[synth]` the figure at the height a generator picks for itself, 1.8970 m against a spec of 1.700, 11.588 percent, accepted. `model.glb` reads 1.6999997, which only rigging made true | `[mut]` the same vertex data with the node's scale taken out of the file, 170 m and 9900 percent. `model.glb` cannot serve: its own vertex coordinates are already in meters, so a local read is right there by accident | `mesh.height_percent`, 25 percent, over the 11.588 of `mesh.survivor-unset.1.json`. The rig's 5 percent band rejects that reading, which is what the positive fixture pins |
| `mesh.facing` | `model.glb`, +Z in glTF Y-up, 0.8 degrees off | `[synth]` the same mesh yawed **180**, which reads -Z, and yawed **90**, which reads X | the horizontal step from the whole body's center to the center of its lowest 5 percent, named by the closest of six axes. **Not the foot island**, see the correction below |
| `mesh.stray_object` | `model.glb`, only `char1` | `[synth]` a second mesh node named `Icosphere`. **Corrected in T2:** the committed `model.glb` at HEAD holds 26 nodes, one mesh, `char1`, and one primitive, so it carries no `Icosphere` and cannot serve. `bare.glb` was not downloadable either, so the negative stays synthetic | the `meshes` allowlist |
| `mesh.budget` | `model.glb`, 54,864 triangles under 300,000 | `[synth]` a strip of 300,001 triangles | `model.glb` reads 54,864 tris against the spec's 30,000 target, which is a remesh hint and not a gate |
| `mesh.uv` | `model.glb`, one tile in [0,1] | `[synth]` a UV at 1.4, at -0.2, and a NaN | 0 out-of-bounds on `model.glb`, counted **as delivered**, before the weld: a seam is one position carrying two coordinates |
| `mesh.texture` | `model.glb`, 0 primitives with no base color image | `[synth]` the same GLB with its material's base color stripped | rigging's own precondition |
| `mesh.cleanup_effective` | the stand-in run: holes 171 to 73 and pieces 7 to 2, so 75 lt 178 | `[synth]` a fixer stub that wrote its input back, 6 lt 6 fails. Plus `[synth]` a class that arrived clean, which must not refuse a fixer that worked | the before and after pair. Non-manifold and self-intersections are both excluded and both have a ceiling of their own: see T10, correction 1 |
| `mesh.mirror`, `rig.mirror_length`, `rig.mirror_direction` under `symmetry: false` | all three report `skipped` on the declaration, and every segment still reports | `[art]` the committed rig, which breaks both `rig.*` rules while the flag is on, and `[synth]` the lopsided mesh, which still fails `mesh.mirror` while it is on | the flag, on all four combinations of it and `cleanup`, through `cargo art check` |
| `rig.bone_set` | `[art]` the committed rig, which carries all 24 after T5's rename | `[art]` `crates/xtask-art/tests/fixtures/humanoid_before_rename.glb`, the same file before it. Measured: 3 of 24 names absent | the 24 committed names, each present exactly once |
| `rig.parents` | `[art]` the committed rig, where all 23 hang as the profile says | `[art]` the pre-rename fixture, `Spine` above `Spine02`. Measured: 4 bones hang wrong, `Spine`, `Head`, and both shoulders | the committed parent map |
| `rig.single_root` | `humanoid.glb`, where every joint descends from `Hips` | `[synth]` `LeftUpLeg` parented outside `Hips`, which takes its whole branch with it, 4 bones | `humanoid.glb` |
| `rig.child_axis` | synthetic conformant rig | `[art]` the current rig, whose `Hips` +Y points out of the left hip. Measured: `Hips` 97.618 deg, `Head` 26.002 deg and `Spine2` 10.157 deg, every other bone under 0.01. **`Spine2` is T5's**: before the rename four `[profile.tails]` rows named bones the rig did not have, so nothing had ever measured the spine chain | per bone, from joint positions. `[synth]` a joint moved without its parent, for the metric itself |
| `rig.mirror_length`, `rig.mirror_direction` | `[synth]` an exact mirror | `[art]` the current rig, worst 3.68 percent and 2.16 deg, both on the `Foot` segment. The 1.45 deg the audit gave is the `ForeArm` pair | measured now: length 0.95 to 3.68 percent over six segments a side, direction 0.08 to 2.16 deg |
| `rig.world_height` | `humanoid.glb`, 2.05 percent out, confirmed at 2.049 | `[synth]` a rig scaled by 100 at the object node, which reads 9900 percent and trips nothing else | 5 percent tolerance, against `spec.subject.height_meters` |
| `rig.humerus_angle` | a conformant rig at 40 | `[art]` the current rig at 59.1 left and 59.3 right, 19.1 and 19.4 out | target 40, tolerance 15 |
| `rig.elbow_bend` | `[synth]` a conformant rig, whose arm is one straight line, reading under 1e-4 | none: it records rather than gates, so it has no verdict to get wrong. What is pinned instead is that a bent elbow stays `info`, on `[art]` the committed rig at 24.0 and 23.5 deg | 180 deg, the ceiling a recording rule reads against. T12 measured 18.5 to 28.0 across three rigs |
| `rig.bind_deviation` | `humanoid.glb`, worst 4.1 deg up the root chain | `[synth]` a rig rolled 90 deg onto its side | the 75 deg band. Measured per step of the root chain, against the up axis, which is the only canonical direction that exists before the aim table |
| `rig.facing` | the current rig already faces +Z in glTF, which is minus Y in Blender, 8.1 to 9.0 deg per foot | `[synth]` the rig yawed 180, and `[synth]` a foot pointing along the up axis, where the facing is undefined and reported as one | the bake camera's forward. Per foot, so one foot on backwards cannot average away |
| `rig.up_axis` | `humanoid.glb`, 3.0 deg off Blender +Z | `[synth]` the same rig pitched 90 deg, which is the shape of an export with no axis conversion | `humanoid.glb`. The root chain end to end, named by the closest of six axes |
| `rig.object_transform` | `humanoid.glb` and `model.glb`, whose bones carry a bind-pose action while the object carries none | `[synth]` a rig carrying an action on the object | both committed rigs read 0 channels above the skeleton |
| `rig.aim_table` | `[synth]` a conformant rig, worst 33.7 deg, read in the `mixamo` convention it is named in | `[art]` the current rig, whose `Hips` axis points out of a hip socket. Measured: 97.8 deg against a band of 75, and every other role inside it. `[synth]` one bone's own axes turned 90 deg while every joint stays put, which no other rule can see | per role, both rigs' rest aims against the table: ours worst 34.9 deg (`right_hand`), a Mixamo FBX worst 45.01 (the arms), measured by hand on an uncommitted download, and `Head` 30.06, inside the band |
| `rig.names_standard` | `[art]` the committed rig, renamed in T5 | `[art]` the pre-rename fixture, which names `Spine01`, `Spine02` and `neck` | the Mixamo name list. This rule reports the names nothing can map, and `bone_set` reports the roles that are missing |
| `gltf.validator` | every shipped GLB | `[synth]` a GLB with an injected NaN | the four committed GLBs |
| `source.posture` | `strafe_left.fbx`, head 34.927 to 35.851, spine lean 11.740 to 12.030, arm swing 124.468 to 139.100, at frames 1, 11 and 21. `[art]` `run.glb` in `crates/xtask-art/tests/fixtures/fetch.run.1.json` | none, `info` only. `[synth]` a clip whose roles the reading needs and which does not have them is left out rather than guessed | the three Mixamo clips. **Read as joint directions, not bone axes**: Mixamo's head bone points 1.027 degrees off vertical while the joint above it leans 34.927, so a bone axis would miss fact 17's hunch entirely |
| `source.fps_declared` | `strafe_left.fbx` at 30, `run.glb` at 24 | `[synth]` `source_fps` 24 against a 30 fps file, which reads 6; and `[synth]` a clip with one key, which has no spacing and reports undefined rather than a NaN | the three clips. Read from the **key spacing**, not from the scene: an FBX keys whole frames and the importer sets the scene from the file, and a 30 fps glTF read at 24 lands its keys 0.8 frames apart, so one formula covers both |
| `source.traveling`, `source.in_place` | `strafe_left.fbx` at `travels: true`, **2.3117 m** of hip travel; `run.glb` at `travels: false`, 0.0000 m | `[synth]` **both directions**: an in-place export declared `travels: true`, and a traveling export declared `travels: false` | symmetric on 0.02 m, so a mistyped flag fails either way. Where the hips **end up**, horizontally: a run cycle in place sways 0.028 m sideways and comes back exactly, and calling that travel would declare every in-place clip a traveling one. The excursion is `source.wander`, beside it |
| `source.wander` | measured on the hips at every frame: `idle` **0.0112 m**, `run` **0.0276**, `walk_back` **1.2712**, `strafe_left.fbx` **2.3117**. `[art]` `run.glb` in `crates/xtask-art/tests/fixtures/fetch.run.1.json` | none, `info` only. `[synth]` a path that goes 0.6 m out and comes back, where `source.traveling` reads 0 and this reads 0.6 | the four clips above. This is the only boundary that can read the excursion at all: the bake pins the horizontal axes onto the first frame before `clip.root_travel` sees them, and `--keep-root-motion` reports both bake rules as `skipped` |
| `clip.swing` | the synthetic cross-rig fixture, and the new output on the three Mixamo clips at 4.1e-5 to 7.1e-5 deg, a hand measurement | `[synth]` a 3 deg swing injected into one role, and `[synth]` the source read one frame out, which fires on every role. `[art]` the shipped `strafe_left.glb`: measured absolutely against the vendor file its worst role is 97.797 deg and its wrists are 76.154 and 78.216, **reproduced exactly by T6's implementation**. It cannot be committed, so the CI negatives are the two synthetic ones | the synthetic cross-rig fixture, T6, which reads 4.3e-6 deg |
| `clip.twist` | the same fixture, and the three Mixamo clips at 11.411 deg, a hand measurement | `[synth]` a 90 deg twist **post-multiplied in the bone's local frame**, `q @ Quaternion((0, 1, 0), radians(90))`, on `LeftUpLeg`. The same test asserts `clip.swing` stays under its limit, which is what proves the injection is a twist and not a yaw. Pre-multiplying by a world +Y rotation would yaw a downward thigh and fire `clip.swing` instead. Plus `[synth]` 16 deg and minus 16 deg, one degree past the limit either way around, which fail beside 14 deg, which holds; and `[synth]` minus 90 deg, which reads 90 and is the control on the rule reporting a size rather than a direction. The shipped clips cannot serve: `rotation_difference` is pure swing, so they carry our rest twist unchanged (fact 2) and this rule reads 0.073 on them | the synthetic cross-rig fixture, T6, which reads 3.5e-6 deg |
| `clip.fps_grid` | the three committed clips at `source_fps` 24, worst **9.5e-7** frames, which is the `f32` a GLB stores key times in | `[art]` the shipped `strafe_left.glb` in a 24 fps scene, range 0.8 to 16.8. In CI, the same shape on a clip that may be redistributed: `run.glb` read at 30, where 15 of its 21 keys land off the grid and the worst reads 0.5. Plus `[synth]` a declared rate of 0, which is undefined rather than infinite | `run.glb`. Measured at two sites: the retarget reads the action, which is the only place an off-grid **import** is visible, and `check/clip.rs` reads the delivered file's own key times, which is where a resampling **export** would show |
| `clip.fps_grid.range` | the three committed clips, 0 frames | `[synth]` 21 keys at 0.8 to 16.8 sampled at frames 1 to 17, which reports the **4** frames the rounding dropped | the frames the retarget samples against the source's own key times. Not the scene's render range: the glTF importer sets none, so that reading fires on Blender's 1 to 250 default |
| `clip.object_transform` | the committed `run.glb` against `humanoid.glb`, exactly 2 subjects: `Armature` and `skin_carrier` | `[synth]` the same clip with the armature's 0.01 scale applied, which is what `transform_apply(scale=True)` leaves, **and** `[synth]` a translation channel on the armature object, which the static reading alone cannot see | the armature scale, byte identical at `0.009999999776482582` across six exported GLBs |
| `clip.root_travel` | the three committed clips after the new strip: **1.8e-9 to 3.7e-9 m** on both horizontal axes, which is the `f32` an F-curve stores | `[art]` the shipped `strafe_left` output under the strip this replaces, which reads **0.0428 m on X** and 0.0170 on Y. Plus `[synth]` the same numbers through `framing.root_travel` | `run.glb`. Two axes, not three: the strip keeps the up one, which is `clip.root_bob` |
| `clip.root_bob` | the four fitted clips: **0.0089 m** (`idle`), 0.0377 (`strafe_left`), 0.0391 (`walk_back`), 0.0535 (`run`) | `[synth]` a root sunk **0.3 m**, which is the shape of the 0.2911 m the old strip left on Z after pinning the root's own channels 0 and 1 | the same four readings. 0.15 m sits 2.8x over the worst of them and 1.9x under the sink it has to refuse |
| `clip.floor_snap` | the three Mixamo fits, at both sites. The retarget's own evaluated pose reads **9.3e-9** (`walk_back`), 1.5e-8 (`strafe_right`) and 4.3e-8 m (`strafe_left`); `check/gltf_clip.rs` on the delivered file reads **3.9e-8** (`strafe_right`), 1.3e-7 (`walk_back`) and 6.7e-7 m (`strafe_left`). The refit of `run.glb` reads exactly 0 at the retarget, and the committed `run.glb` reads **0.0016536 m** in Rust | `[art]` the committed `idle.glb`, fitted before the snap existed, whose lowest toe hangs **0.0773 m** above the datum, and which a refit brings to 9.3e-9 m. Plus `[synth]` a delivered GLB whose root is keyed **0.06 m** below where its own rig rests, and the pair either side of the limit at 4 mm and 6 mm. `[mut]` the same retarget with `lift_root` removed, which reads **0.0599 m** on `strafe_left` | the lowest ground joint's frame, against the rest height the snap aims at. **Not zero**, and not the sole either: see correction 1 |
| `clip.stride` | the three Mixamo fits: `strafe_left` **2.0378 m** against a source travel of 2.3117 sized by 0.8815, `strafe_right` 2.5477 against 2.8901, `walk_back` 1.2465 against 1.4140. The retarget reads **4.6e-6** (`walk_back`), 7.6e-6 (`strafe_right`) and 1.3e-5 percent (`strafe_left`); Rust on the delivered file reads **1.4e-5**, 1.5e-5 and 2.4e-5 percent on the same three | `[mut]` the same retarget with `scale_translation` removed, which reads **13.4409 percent** on `strafe_left` at both sites. Plus `[synth]` a fit whose root keys are scaled by 1.05, which reads 5.0 percent in Rust and in Python, and `[synth]` `travels: true` on a source that never moves, which is undefined rather than a division by nothing | T5's hand measurement, 2.3117 m times 0.8815 giving **2.0378 m**. `travels: false` reports `skipped` on the declaration, which the refit of `run.glb` records. What the two sites prove and cannot prove is correction 7 |
| `clip.stride_ratio` | the same three at **0.8815161761312765** in the retarget and **0.8815163067471855** in Rust, off 0.3578832274114926 m of our femur against the vendor's 0.40599429901464934. The refit of `run.glb` records **1.0000204384738902**, which `crates/xtask-art/tests/fixtures/retarget.run.1.json` carries and a unit test pins to 1e-9 | none, `info` only, per the Terminology exemption | the two `stride_segment` joints of each rig at rest. Not 1 on a refit: the rig is exported, imported and exported again, and a GLB stores a joint position as an `f32` |
| `clip.foot_contact.plants` | the three Mixamo fits at both sites: the left foot plants once on each, over frames 10..15 (`strafe_left`), 12..14 (`strafe_right`) and 9..27 (`walk_back`). `[synth]` the standing cross-rig pair, both feet, at both sites | `[synth]` the cross-rig pair as it stands, whose feet ride along with a root crossing 2.04 m in a third of a second, which comes to rest nowhere. `[art]` the **right** foot of all three Mixamo fits, at **0** runs: see correction 3 | `ge 1` per foot per cycle. The `[synth]` in-place clip the design named is a `skipped` instead, because `travels` switches the rule off |
| `clip.foot_contact.skate` | the three Mixamo fits' left foot: **0.0000 m** at the retarget on all three, and 0.0000, 0.0021 and 0.0000 in Rust on the delivered files | `[synth]` the standing pair whose planted foot is translated **5 cm** during stance, which is slow enough to still read as contact. `[mut]` the same three fits with the lock removed, which read **0.0207** (`strafe_left`), 0.0148 (`walk_back`) and 0.0027 m (`strafe_right`), all inside the published 2.5 cm: what the mut proves is what the lock removed, and the 5 cm synthetic is what makes the rule fail | the pair 0.0207 against 0.0000 m. The 2 mm the delivered `strafe_right` reads is correction 5 |
| `clip.foot_contact.penetration` | the refit of `run.glb`, which clears the floor by **0.0074** and 0.0005 m, and the vendor's own `strafe_left.fbx`, which reads **0.0000 m** at every planted frame and 0.0038 at worst | `[synth]` the standing pair keyed **2 cm** under its own floor, plus the pair either side of the limit at 4 mm and 6 mm. `[art]` the **left** foot of all three Mixamo fits, at **0.0200 to 0.0203 m**: see correction 4 | 5 mm, on the two sole points of each foot. The vendor file reading 0.0000 at every plant is what says the sole model is the right one |
| `clip.loop` | `run.glb` at **0.000** deg and `idle.glb` at **0.487** | `[art]` `walk_back.glb`, which reads **6.910** on `LeftForeArm` and breaks on nine of its 24 bones. That is the hitch `library.ron` has recorded in prose all along and which nothing could measure until now. Plus `[synth]` a whole cycle against the same cycle cut one frame short | `idle.glb` and `run.glb`, 2.0 deg today. Per joint, on the local rotation, so one wrong hips reports once rather than dragging every bone below it into the count |
| `clip.interpolation`, `clip.reference_pose_key` | the recorded report of the `run.glb` refit, committed as `crates/xtask-art/tests/fixtures/retarget.run.1.json`: 44 findings, no error | `[synth]` in CI, on `clip.py`: a Bezier key, a pose not held at the ends, and a key off either end of the range. Plus `[mut]` two Blender runs, one with the LINEAR and CONSTANT pass removed and one with a pose keyed outside the source's range, each firing its own rule on all 22 bones and leaving the other at `info`. Plus `[synth]` three reports the runner refuses: an unpublished rule id, a limit of its own, and a defect filed as `info` | `run.glb`. **The reference pose is never keyed at any frame**, so the rule measures keys outside the source's frame range rather than at frame 0. See the correction below |
| `bake.frame_count`, `bake.non_empty` | the recorded run: 240, 320 and 288 frames rendered of the same, and 4.1164 percent of a canvas at the emptiest | `[synth]` one frame deleted, which reads 1 missing and names `w 01`, and one fully transparent, which reads 0.0000 percent | directions x sampled frames, alpha coverage. The rendered set is gitignored, so the readings are the recorded run in `crates/xtask-art/tests/fixtures/bake.survivor.1.json` and every negative is a synthetic 64 px frame set |
| `bake.in_frame`, `bake.pivot` | the recorded run: 89, 65 and 58 px of inset, and 1 px of reflection error over 424 opposite pairs | `[synth]` a pose clipped at the border, which reads 0 px, and a frame offset 20 px, which reads 20 | 1 px of inset, and the reflection between opposite directions. **The ground line cannot serve**: correction 1 |
| `bake.forearm_roll` | spec field 0.0 | `[synth]` the field set to 30 | `eq 0` |
| `bake.sampled_frames_are_keys` | the recorded run: `idle` renders 15 frames of the 47 its action keys, `run` 20 of 21, `walk_back` 18 of 23. Green by construction, because each clip keys every integer frame of its own range: correction 2 | `[synth]` an action with every other key deleted, which reports the 3 frames of 6 that nothing keyed | `framing.sampled_frames` already rounds to integers. **Any channel, not every channel**: correction 2 |
| `bake.landmark_golden` | the six committed goldens, all 432 landmarks at 0 px | `[synth]` one arm rotated 30 deg before projection, which moves a wrist 21 px; `[synth]` a golden of another set of joints, which is undefined; and no golden at all, which is an error | 3 frames x 2 directions per clip. The projection's own calibration is two known answers plus all six goldens read for a body the right way up: correction 3 |
| `atlas.frame_count`, `atlas.trim_boxes` | the three committed atlases and the manifest the game loads, 0 defects each | `[synth]` a manifest claiming one extra frame, which reads the 16 cells with no rect, and a box one pixel outside, which reads 1 | the committed atlases |
| `atlas.manifest_schema` | the same manifest, which `sprites::parse` loads as three animations | `[synth]` a missing field, which also leaves the other two rules undefined on every animation, plus a table of every refusal `sprites::parse` has, one case each on a two-frame manifest, each caught as an error by some `atlas.*` rule | the committed manifests |

### Corrections T3 made to this document

Each one is a place where the design as written would have produced a precise,
wrong number, which is the failure this whole document exists to stop.

1. **A skinned mesh is not placed by its node chain.** The design said "take
   it to world through the glTF node chain". The glTF specification says the
   node transform of a skinned mesh must be **ignored**, because the skin's
   joints carry it. `bare.glb` and `clean.glb` are unskinned, so the node
   chain is right for them, but the only calibration asset available is
   skinned. Read through the node chain, `model.glb` measures 0.017 m, and
   `parry3d`'s f32 queries at that scale report 17,549 self-intersecting
   faces against a true 861. `check/gltf_mesh.rs` therefore reads
   `joint world * inverse bind matrix` per vertex where a primitive is
   skinned, and the node chain where it is not. Fact 20 records it.
2. **`mesh.non_manifold` had no Test Plan row and no published limit.** The
   Architecture Overview names it on `bare.glb` and `mesh.non_manifold_post`
   on `clean.glb`, which are two rules on two assets: the fixer makes the
   count **worse**, 8 to 13, which is also why `cleanup_effective` excludes
   it. T3's success criteria say "every `mesh.*` row except
   `non_manifold_post`", which presumes the other row exists. Both rows are
   in the tables now.
3. **`mesh.quads` cannot be `info`, never `error`.** As specified it is a
   rule that structurally cannot fail, which is failure shape one from "How
   do we know a gate can fail". What the rule can honestly measure is whether
   every primitive is triangles, because glTF cannot express a quad at all. A
   primitive of points or lines is a primitive **every other mesh rule
   silently did not measure**, so it is an error with a negative fixture, and
   the other rules report `undefined` rather than zero when it fires.
4. **A self-intersection is measured by intersection, not by penetration
   depth.** Two triangles have no thickness, so they cross in a line of zero
   volume and EPA reports a depth of 0. Two visibly interpenetrating boxes
   read **zero** faces that way. `check/mesh.rs` uses `parry3d`'s boolean
   triangle query, so a pair that touches counts: after welding, two faces
   that meet and share no vertex are two different parts of one surface in
   contact, which is the fused geometry the design names as the cause of
   unreliable weight painting. This is why the number is 861 and not the 16
   fact 8 recorded, and the 16 could not be reproduced at any threshold: 0 to
   1e-5 m of depth gives 41 faces, 1e-3 m gives 14.
5. **`mesh.facing` cannot use the foot island.** The design said "toe-tip
   minus heel of the foot island". The mesh gates run before rigging, so no
   ankle joint exists to measure from, and the feet are not their own island
   on a welded body. The rule measures the horizontal step from the whole
   body's center to the center of its lowest 5 percent, then names the
   closest of six signed axes, exactly as `rig.facing` does. On `model.glb`
   that slice is 1,670 vertices sitting 4.4 cm forward and 0.6 mm sideways,
   so the direction it names is not noise.
6. **The `mesh.world_size` `[mut]` cannot be `model.glb`.** The design says
   "read from the local bbox, 170 m". `model.glb`'s own vertex coordinates
   are already in meters, so reading them locally gives 1.7 m and is right by
   accident. The fixture is synthetic: the same 100x vertex data written
   twice, once under the 0.01 node and once under an identity node.
7. **Every synthetic negative is scaled to beat its published limit.** A
   single box with a missing face has 3 boundary edges against a limit of
   200, so it passes. Each count negative is the same defect laid out n
   times, and every one is asserted with the number it produces.
8. **`mesh.printability` reports one subject, not two.** A boolean and a
   count cannot share one `le` limit: `is_watertight: false` beside a low
   edge count would pass. Meshy's `is_watertight` is its own summary of the
   same `non_manifold_edges` count, so the rule measures the count and states
   the verdict in the message.
9. **`check/gltf_mesh.rs` is a new file** beside `check/mesh.rs`, the same
   split `gltf_world.rs` and `rig.rs` already use: one module owns the
   representation, the other owns the rules. `gltf_world.rs` now exposes
   `world_nodes`, so the node chain has one owner that both families read.
10. **A shortfall of inverse bind matrices is refused.** The specification
    allows the accessor to be absent, which means identity. Fewer matrices
    than joints is malformed input, and part of this asset family's 0.01
    scale lives in the matrix, so filling the gap with identity puts every
    vertex on that joint 100x out and says nothing.
11. **`mesh.printability` reports on every run**, as unavailable, because
    `cargo art check` calls nothing and the model stage is where the response
    comes from. A gate that goes quiet when it cannot run is the failure
    shape this design opens with, so the rule is not allowed to be absent
    from a report.

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
- **Aim table validation:** a missing row, a row no convention maps, and a
  broken mirror pair. The band check against a rig is Rust's, per rule four,
  as `rig.aim_table`.
- **Foot planting** in `plant.py`: a toe path with two known plant runs, a
  path with none, the same path at 8 and 30 fps giving the same runs, and the
  vote width odd and at least 3 at both rates. The lock holds a run's XY to
  1e-9 and leaves the frames outside its ramps untouched; the leg solve
  reaches a reachable target to 1e-6 and reports an unreachable one as the
  distance it fell short by. `check/foot.rs` runs the same paths and asserts
  the same numbers, so the two implementations are pinned to one answer.

**Unit, `cargo nextest --test unit`, in CI.** Every row above, plus:

- `check/gltf_world.rs` against hand-written fixtures with a nested hierarchy
  and a non-identity parent scale.
- `check/clip.rs`'s split against the same two numeric cases as the Python
  side, so the two implementations are pinned to one external answer.
- The `print/analyze` parser against a recorded response.
- The rigging body: `model_url` present and `input_task_id` absent, with the
  cleaned mesh when `cleanup` is on and the bare one when it is off. See T10,
  correction 11.
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
`print/analyze` reports `warning` and the build continues. The four
combinations of `cleanup` and `symmetry` are unit tests instead, through
`cargo art check`: no thread and no channel is involved, so the tier the
design named for them buys nothing.

**End to end, `cargo nextest --test e2e`:** T16 launches Godot headless, loads
every atlas and manifest, and greps the log, because Godot exits 0 on a script
error (`research_godot_ci_e2e_testing.md`). Loading only, never pixels.

**Cross-reference to Out of Scope:** no test asserts color pixels, loads a
quadruped profile, calls Meshy's paid repair, checks a concept arm angle,
changes a sprite rate, or asserts a strict T-pose bind.

### Corrections T4 made to this document

1. **`parents` was two tables under one word.** T2 had already taken it for
   `[profile.parents]`, our rig's real bone hierarchy. The retargeting one
   ships as `[retarget_chain]`, keyed by role, and `optional` and
   `fingerprint` ship as `optional_roles` and `[fingerprints]`, which is what
   TOML lets a list and a per-convention table be.
2. **An aim row is a direction, not three euler degrees.** The sketch read
   `hips = [ 90, 0, 0 ]` with the comment "degrees XYZ", while the
   Terminology calls the table a world direction. A mirror row has to be an
   **exact** reflection, and mirroring a rotation is not negating one euler
   component, so an euler triple cannot carry that check. The rows are
   directions, the two readers normalize them, and every row stays whole
   numbers: `hips = [0.0, 0.0, 1.0]`, `left_arm = [1.0, 0.0, -1.0]`.
3. **The band check cannot be a load-time refusal, and it is not Python's.**
   Loading a table has no rig in hand. Reading a rig's rest aim needs glTF
   world transforms, which `check/gltf_world.rs` owns, so rule four puts it in
   Rust as `rig.aim_table`, one finding per role, printed by `--list-rules`.
   The two structural checks stay on both sides, because each side has to
   refuse the table it reads: `skeleton.py` for the transfer, `check/aim.rs`
   for the gate.
4. **"Both rigs" is one rule run per rig.** Ours is measured at the rig stage.
   A source rig is measured when its motion arrives, in the convention it is
   named in, which is why the rule takes that convention as an argument.
5. **A rest aim is the bone's own axis, not the direction to its child.** The
   table prescribes where a bone points, and `rig.child_axis` already
   measures the joint-to-child direction. Reading the same thing twice would
   have made `rig.aim_table` a second copy of that rule, and it would have
   left every hand and toe unmeasured, because a leaf has no child to point
   at. Those are exactly the bones the code this replaces skipped.
6. **The arms aim 45 degrees below horizontal, and that is measured.** The
   aim has to sit inside `max_bind_deviation_degrees` of both rigs. A T-pose
   table puts our hands 76.15 and 78.22 degrees out, past a band the profile
   says is wide enough for an A-pose, and `test_aim.rs` pins those two numbers
   so that widening the band for another rule cannot make a T-pose table
   legal. At 45 degrees our rig is worst at 34.86 and a Mixamo rig at 45.01.
   The one row left outside is our `Hips`, at 97.80 degrees, which is fact 1's
   sideways hip axis and decision 11's negative control.
7. **Every Mixamo figure here is a hand measurement, not a test.** They were
   read off `art/staging/downloads/strafe_left.fbx` in Blender 5.2.1, and
   `art/staging/` is gitignored, so no test can open that file and CI cannot
   re-derive them. Our own rig's figures are all asserted in `test_aim.rs`
   against the committed GLB. **T6 corrected the sentence that followed**: no
   CMU clip is committed, so a source rig enters the suite as the synthetic
   cross-rig fixture in `clips.rs`, and `rig.aim_table` still has no second
   real rig to measure.
8. **A four number row was accepted and its fourth number dropped.** Serde
   read `[f64; 3]` out of a longer TOML array without a word. Both readers
   now refuse any row that is not three numbers.

### Corrections T5 made to this document

1. **`transfer.py` cannot import `mathutils`.** The Logic sketch is written in
   `Vector` and `Quaternion`, which only exist inside Blender: `mathutils` is
   not a dependency of this repository and the venv carries stubs alone. A
   `transfer.py` that imported it could not be unit tested, which is the
   module's whole reason for existing, so it carries about 90 lines of its own
   vector, quaternion and 4x4 affine math over plain tuples. The alternatives
   were weighed and are on the record. **The PyPI `mathutils`** is a third
   party extraction of Blender's C module, so the math would be pinned to a
   package nobody in this project maintains. **numpy** ships inside Blender
   and covers the affine half, but it is not a dependency of the venv either
   and it carries no quaternion type, so the swing-twist split would still be
   ours. **scipy**, whose `Rotation` does carry one, does not ship with
   Blender at all. Owning about 90 lines beats owning a dependency that
   answers half the question, and it keeps the "no new dependency in a
   headless build" the retargeter decision claimed.
2. **The reference pose occupies no frame, so `clip.reference_pose_key`
   measures something better.** "No key at the reference frame" and the Test
   Plan's "no frame 0 key" both assume the reference pose is posed and keyed.
   It is not: it is computed algebraically from the two rigs' rest matrices,
   and the target rig is never posed at all. Frame 0 is also not free. The
   three Mixamo clips run frames 1 to 21 and the committed Meshy clips start
   at 0, so a fixed reference frame of 0 would delete a real frame from a
   refit. The rule therefore counts keys **outside the source's frame
   range**, which still catches retarget_bvh's frame-0 T pose and catches a
   key past either end. It is a range and not the source's own key set: the
   transfer resamples every frame between the two ends on purpose, so a key
   the source does not have is the output working as intended. For the same
   reason **requirement 9 removes no key**: the output's frame set is exactly
   the source's, and the sentence in the two-rule decision saying the ranges
   "can differ by one" is wrong.
3. **A swing metric cannot see a wrong aim table.** Aiming both rigs at the
   same table makes every offset a pure twist about the bone's own axis, which
   cannot move where the bone points, so `clip.swing` reads 0 whatever the
   table says. Measured: feeding the table where a reference pose belongs
   leaves the worst swing at 1.2e-14 degrees while moving the offsets by up to
   160.970, and a sign error on one mirror row re-rolls that arm by 64.923
   degrees and reads 2e-14 on swing. This is the strongest argument for the
   two-rule verifier there is, and it is why two of the three known-answer
   assertions are on the **offset** rather than on the output.
4. **`Offset(LeftUpLeg)` is 176.47 degrees on the real pair, not 174.** 174 is
   the rest roll difference between the two legs (fact 3). The offset is
   measured after the whole chain is aimed, and our `Hips` needs a 97.8 degree
   correction that every child's reference frame carries. The design's own
   number is kept as a known-answer test on an isolated two-bone pair, where
   174.00 is provably the answer and nothing else can contribute.
5. **The absolute accuracy is 0.000181 degrees, not "about 0.1".** Worst
   absolute per-bone swing against the vendor file, over all 22 roles: 0.000181
   on `strafe_left`, 0.000184 on `strafe_right`, 0.000196 on `walk_back`. The
   figure is glTF's f32 storage, not the math. The prototype's 0.1 came from
   reading poses back through a dependency graph. **Every one of these three
   is a hand measurement, not a test**, read in Blender 5.2.1 against
   `art/staging/downloads/*.fbx`: that directory is gitignored, so no test can
   open those files and CI cannot re-derive the numbers. The same is true of
   the `clip.swing` positive column in the Test Plan. What CI does hold is the
   same invariant on a synthetic pair, at 1e-9 degrees, in
   `test_transfer.py`.
6. **Two of T5's four acceptance postures cannot be reproduced, the cause is
   measured on both rigs, and only our half of it is ours to fix.** A
   joint-chain lean is the bone's own direction only on a rig that passes
   `rig.child_axis`, and neither rig does. The transfer preserves each bone's
   own axis exactly, so those two leans come out shifted by the rest-geometry
   difference: neck lean 17.3 to 20.3 against the source's 34.0 to 37.0,
   spine lean 26.9 to 31.6 against 4.5 to 8.5. The two elbow ranges, whose
   bones do point at their children on both rigs, are reproduced to 0.002
   degrees. **The refit is the control:** the same clip on the same body
   reproduces every one of the four to 0.001 degrees, which places the
   difference in the rigs and not in the transfer.

   The residual splits by rig, and the two halves have different owners.

   | Whose | Measured | Who closes it |
   |---|---|---|
   | ours | `Hips` 97.615 deg off the direction to `Spine`, `Spine2` 10.157, `Head` 26.002 | **T15**, which regenerates the rig. `rig.aim_table` already reports the `Hips` row as an error at 97.801 against a band of 75 |
   | the vendor's | Mixamo's `Neck` axis 16.933 deg off the direction to its own `Head`, its `Hips` 7.051 off the direction to `Spine` | **nobody.** It is not our skeleton to regenerate |

   So T15 does **not** close the neck. After it, our conformant `Neck` will
   point exactly where their non-conformant `Neck` points, which is 16.933
   degrees off the direction to their own `Head`, so our head joint will
   still lean about 17 degrees differently from theirs on every Mixamo clip.
   That is the correct output of a correct transfer between a conformant rig
   and one that is not, and the only honest thing to do with it is report it:
   T7 adds **`source.child_axis`**, an `info` rule beside `source.posture`,
   measuring each source bone's own axis against the direction to its mapped
   child at fetch time. It carries no negative, per the Terminology
   exemption, and it goes in the audition artifact so a clip's own rest
   geometry is on record before the pipeline spends anything on it.
7. **The rename closed three rules, and the seven-rule negative moved to a
   fixture.** `rig.names_standard`, `rig.bone_set` and `rig.parents` pass on
   the committed rig now, so their `[art]` negative is
   `crates/xtask-art/tests/fixtures/humanoid_before_rename.glb`, the same file
   before the rename. **Five** rules still reject the live rig and all five
   are geometry: `rig.child_axis`, `rig.humerus_angle`, `rig.mirror_length`
   and `rig.mirror_direction` from `check/rig.rs`, plus `rig.aim_table` from
   `check/aim.rs`, which shares the prefix and is easy to miss when counting
   one file. `cargo art check` prints 14 defects across the five. The rig now
   resolves 138 measurements against 127, because 11 of them named bones it
   did not have.
8. **A rename is a JSON-chunk edit, which is why the mesh limits survive it.**
   glTF addresses a joint by node index, so a bone's name is one string in one
   place. Rewriting the JSON chunk and leaving the BIN chunk byte identical
   keeps every vertex, accessor and inverse bind matrix, and every provisional
   `[profile.mesh]` calibration still reads what T3 measured. The sequence is
   in `art/skeletons/README.md`.
9. **The three committed atlases are now older than the clips.**
   `project/assets/characters/survivor/` holds `idle.png`, `run.png` and
   `walk_back.png`, baked before the rename from the clips as they were.
   Refitting the clips does not rebake them, and T5 does not: the sprites are
   unchanged art from a source that has moved, and every one of them is
   regenerated in T15 along with the rig. Nothing between here and there
   reads an atlas against its clip, which is `bake.landmark_golden`'s job in
   T14.
10. **There are three committed Meshy clips, not two.** `walk_back.glb` is
   `Meshy(action_id: 544)` in `library.ron`, authored on this rig like `idle`
   and `run`, so the rename invalidates it too and it is refit in the same
   change. Every clip in `art/animations/` must be refit, not two of them.
11. **The `[art]` negative for `clip.swing` cannot be committed.**
    `art/animations/local/` is gitignored because Adobe's license allows the
    motion in the game and forbids publishing the file, and the shipped
    Mixamo outputs live only there. Their numbers are recorded in the Test
    Plan instead, measured before the old code was deleted.
12. **The two `clip.*` rules T5 owns are measured in Python, and only the
    reading needs `bpy`.** Rule four sends CI-side measurement to Rust, and
    these two cannot go: the glTF exporter resamples every channel and writes
    its own interpolation, so a Bezier action exports as LINEAR and the
    defect is invisible in the file. What can go, and did, is the counting.
    `clip.py` takes a `Channel` record and returns two counts per bone with
    no `bpy` anywhere, so both rules have a pytest negative that runs in CI:
    a Bezier key, a pose not held at the ends, and a key off either end of
    the source's range. The shell only reads F-curves into `Channel`. The
    rules themselves stay in `check/clip.rs` so `--list-rules` prints them,
    and the runner refuses a report that disagrees with what it printed, down
    to the severity: the comparison decides that, so a defect filed as
    `info` is refused rather than counted as quiet.
13. **`test_framing.py` lost 14 tests, and none of them asserted a function
    against itself.** The deleted-functions table says twelve, "each asserts a
    function against itself". The 14 that go are the tests of
    `bind_pose_mismatch`, `bone_direction_angle`, `loop_mismatch` and
    `rotation_angle`, and every one carries a literal expected value. The
    genuinely self-referential tests in that file recompute
    `Framing.ortho_scale`, which "Kept untouched" keeps, so rewriting them is
    not this task's.
14. **The stray `Icosphere` is Blender's glTF importer, not the art.** Fact 8
    records it as an object in `model.glb` and T3's correction says the file
    holds none. Both are right: an empty scene plus one import gives a
    42-vertex unparented `Icosphere` beside the real mesh, for `humanoid.glb`
    and for `model.glb`, neither of which names one anywhere in its JSON. That
    is where the audit's island count of 8 came from against T3's 7.
15. **The femur ratio landed here, not in T8, and the two roles are data.**
    T5's acceptance measures root travel, and travel is only right once the
    metric is right, so requirement 4's first half ships with the transfer:
    2.3117 m of source travel times a femur ratio of 0.8815 gives 2.0378 m,
    measured on the output. Which two roles those are is skeleton data and
    not glue, so the skeleton file gains `stride_segment`, refused at load
    time if it names something that is not a role or one joint twice. A
    quadruped's stride is not a human femur. T8 keeps the floor snap and the
    2 percent travel band.
16. **An undriven bone had to be put back at rest, and a fractional frame
    range had to be refused.** Two things the transfer's docstring claimed
    and nothing enforced. "Held at its rest transform" is only true if
    something clears the pose: `humanoid.glb` arrives carrying its own
    bind-pose action, and replacing an action does not undo the pose it left
    behind, so `write_keys` now sets `matrix_basis` to identity on every bone
    first. Measured: a bone left 30.00 degrees off rest comes out at 0.00.
    Today only two roleless leaves are undriven, and the first
    `optional_roles` gap would have made it a silently wrong arm. And the
    frame range was rounded, which is fact 6 hiding: the output of a 30 fps
    clip read back in a 24 fps scene spans 0.8 to 16.8, and rounding it to 1
    to 17 drops four frames without a word. The retarget refuses a range that
    is not whole within 1e-4 of a frame and names T7, which sets the scene
    rate from `source_fps` and reports `clip.fps_grid`.
17. **`findings.attempt()` is new.** A script builds its own Findings and every
    Finding carries an attempt, and T1 exposed no way to read it: the header
    parser was private. It is three lines, and it keeps the attempt coming off
    the one path the runner set.

### Corrections T6 made to this document

1. **`clip.twist`'s reference term was a difference of two absolute twist
   angles, and that is not a rotation.** As published, the rule subtracted
   `twist(out, rest) - twist(src, rest)`. Two angles can only be subtracted
   like that when the two rotations share their swing, which `out` and `src`
   do at every frame, because the offset is a pure twist, and which the two
   **bind poses** do not. Measured on the three Mixamo clips, the published
   form reads **108.805, 107.627 and 107.471 degrees** on the toes and the
   hips of a correct fit, identical to five decimals across three different
   motions, because the quantity is a property of the two rest poses and not
   of the clip. A limit above it would be 120, and the design's own negative,
   90 degrees post-multiplied on `LeftUpLeg`, lands at 95.5 and passes. That
   is a gate that cannot fail on the one fixture it ships with.

   The correction is one term. Both rules now come from one split of
   `relative = source^-1 @ output`, in the source bone's own frame: the swing
   of that rotation is `clip.swing`, and its twist read against the same twist
   at rest is `clip.twist`. The rest term is then a composition rather than a
   subtraction, and the worst reading on the three clips falls from 108.805 to
   **11.411 degrees**. The 174 degrees of convention difference still cancels
   exactly, which was the whole point of two rules.

   **What the remaining 11.411 degrees is, and that it is not a defect.** Our
   rig is A-posed and Mixamo's is T-posed, so the same bone rests pointing 62.5
   degrees apart on the forearms and 76 to 78 on the hands. Aiming both at one
   table swings each bone by a different amount, and a swing carries a roll of
   its own. So the roll the clip ends up with is not exactly the roll between
   the two bind poses, and the gap is that difference. It is 0 when the two
   rigs differ by pure roll, which is fact 3's case for the legs and which the
   synthetic fixture reproduces exactly. T15 regenerates our rig closer to the
   40 degree target and can retighten the limit; it will never reach 0 against
   a T-posed source, and decisions 2 and 6 say we are not re-binding to a
   T-pose to get there.

2. **The calibration is a synthetic cross-rig fixture, not a committed CMU
   clip.** "Commit one small CMU BVH clip" would mean an unattended download of
   third-party data, a license to track for the life of the repository, and a
   calibration nobody here computed. The fixture is
   `crates/xtask-art/tests/unit/clips.rs` instead: our own conformant rig
   against a second one whose bones differ from it by a **roll** about their
   own +Y and a **tilt** about their own +X, both taken from what the real pair
   measures. The legs are 174 degrees apart, which is fact 3, and the arms rest
   62 to 77 degrees apart, which is the A-pose against the T-pose.

   The reason it can serve is that both answers are provable rather than
   measured. Write the vendor's rest as `S_rest = O_rest @ Ry(-roll) @
   Rx(-tilt)` and its motion as `S(t) = O(t) @ Ry(-roll)`. Then
   `S(t)^-1 @ O(t)` is `Ry(roll)`, which cannot move +Y, so `clip.swing` is
   **0**; and `S_rest^-1 @ O_rest` is `Rx(tilt) @ Ry(roll)`, already a swing
   times a twist about +Y, so its roll is `roll` and `clip.twist` is **0**. A
   test measures the pair in `f64` and reads under 1e-9, and the same pair
   through a GLB reads 4.3e-6 and 3.5e-6. **The whole calibration is what the
   file format costs.**

   The real numbers stay hand measurements, labeled the way T4 labeled the
   Mixamo 45.01: read in Blender 5.2.1 against `art/staging/downloads/*.fbx`,
   which is gitignored, so no test can open those files and CI cannot
   re-derive them.

   | Asset | Worst `clip.swing` | Worst `clip.twist` |
   |---|---|---|
   | the synthetic cross-rig pair, in `f64` | under 1e-9 | under 1e-9 |
   | the same pair, through a GLB | 4.3e-6 deg | 3.5e-6 deg |
   | `strafe_left`, hand measurement | 7.05e-5 deg (`right_hand`) | 11.411 deg (`left_forearm`) |
   | `strafe_right`, hand measurement | 4.11e-5 deg (`head`) | 11.411 deg |
   | `walk_back`, hand measurement | 4.64e-5 deg (`left_hand`) | 11.411 deg |
   | the shipped pre-rename `strafe_left.glb` | **97.797 deg** (`hips`) | 0.073 deg |
   | limit | **0.01**, 142x over the worst real reading | **15.0**, 3.589 deg and 31 percent over it |

   The last row of measurements is the one that matters most: T6's
   implementation reproduces the Test Plan's recorded 97.797, 76.154 and
   78.217 on the shipped output to three decimals, having never seen those
   numbers. `clip.twist` reads 0.073 there, which is fact 2 exactly:
   `rotation_difference` is pure swing, so the shipped clips carry our own
   rest twist unchanged and only `clip.swing` sees them.

3. **How the source reaches a Rust gate.** The vendor file is an FBX, the
   `gltf` crate cannot open one, and rule four keeps CI-side measurement in
   Rust with no Blender. So `retarget_animation.py` writes the source clip's
   own world orientations, per role, to
   `art/staging/reports/retarget.<clip>.<attempt>.source.json`, named by
   `Artifacts` like every other run artifact, and `check/clip.rs` reads that
   beside the delivered GLB. The record is built in `clip.py` with no `bpy`,
   so it is unit tested like everything else there.

   **Both sides are brought into Blender Z-up world space before either is
   measured**, and `--list-rules` says so. That is not decoration: a twist
   about a bone's own axis is **not** invariant under a change of world frame,
   so reading one side in glTF Y-up and the other in Blender would give precise
   wrong numbers, which is the failure this design opens with.

   **The output is read from the file rather than from the transfer.** Between
   `transfer.py` and the GLB sit `write_keys`, the travel scale, the
   interpolation pass and the exporter, and nothing else measures any of them.
   `check/gltf_clip.rs` samples the node graph at every key time the file
   carries, so a dropped key is a frame that is missing rather than a value
   quietly interpolated over.

4. **`clip.object_transform` was defined twice over, and neither half is
   true.** "Every object matrix in the output GLB is identity" is false of
   every file this pipeline has written: the armature node carries the
   family's 0.01 scale, in `humanoid.glb`, in all three refit Meshy clips and
   in all three shipped Mixamo clips. Worse, it is backwards: applying the
   object scale is exactly what makes that matrix identity, so the rule as
   written would have passed the mutation it exists to catch. "Rest bone
   matrices byte-match the committed rig" is false too, and measured:
   `LeftToeBase` in `run.glb` reads `13.177001953125` against
   `13.177124977111816` in `humanoid.glb`, a `f32` round trip through
   Blender's importer and exporter worth about a micron in world space.

   What the rule measures instead is that **every node above the skeleton
   carries the transform the committed rig gives it**, compared exactly. That
   comparison is safe because the value is a stored object transform rather
   than a recomputed one: six independently exported GLBs carry the armature
   scale as the identical `0.009999999776482582`. `transform_apply(scale=True)`
   moves that scale out of the object and into the rest geometry while leaving
   every location key byte identical, so it changes this one number and nothing
   else in world space, which is why this is the honest place to look for it.

5. **A clip rule reports its subject as a role, not as a bone.** The
   Interfaces example named a bone, and now names the role. These two rules
   pair two rigs, whose bone names need not agree, and the role is the only
   key both files carry, so the subject is `left_hand` and the message names
   our own bone. `rig.aim_table` already reports roles for the same reason.

6. **A constant offset between the two clips' key times cannot be a defect,
   and the fixture had to change to say so.** Both sides are counted from
   their own first frame on purpose, because a Mixamo clip runs frames 1 to 21
   and a Meshy one starts at 0. So the negative that proves the alignment is a
   **rate** difference, which is fact 6's shape: a 30 fps clip sampled in a 24
   fps scene drifts a frame further apart every frame.

7. **The glTF export has to ask for the armature at rest, in writing.**
   `clip.twist`'s rest term is read off the joints of the delivered GLB, so
   the armature must leave Blender at its rest position and not at whatever
   frame the scene happened to be on. That is the exporter's default and only
   the default, so both `retarget_animation.py` and `strip_animation.py` now
   pass `export_rest_position_armature=True`. Measured on `strafe_left`: with
   the flag on, `clip.twist`'s worst reading is **11.411 degrees** and every
   role holds; with it off, the worst is **113.884 degrees** and 20 of the 22
   roles are errors, while `clip.swing` stays at 7.0e-5 either way. The same
   file read against the `rig.*` rules goes from 13 defects to 18:
   `rig.mirror_direction` on `Leg` from 1.042 to 110.848 degrees, plus
   `rig.world_height`, `rig.bind_deviation` and `rig.facing` on both feet.
   A unit test scans every Blender script and fails on an export that carries
   an armature and leaves the flag to the default.

8. **What the 11.411 degrees is made of, per role.** The limit is 15.0 and
   the worst real reading is 11.411, which is thin. Each number below is one
   role's `clip.twist`: how far the roll our rig ends up with sits from the
   roll its bind pose and Mixamo's call for. They are identical to three
   decimals on all three Mixamo clips, which is the point: the quantity comes
   from the two rest poses and not from the motion, so T15 can retighten the
   limit once the rig is regenerated. Only the roles over 2 degrees are
   listed; the rest, the hips and the left leg among them, are under 0.3.

   | Role | Left | Right |
   |---|---|---|
   | `forearm` | **11.411** | 11.112 |
   | `hand` | 10.382 | 11.407 |
   | `shoulder` | 7.382 | 7.475 |
   | `toe` | 5.615 | 6.766 |
   | `foot` | 4.107 | 6.190 |
   | `arm` | 3.088 | 2.995 |
   | `spine_upper` | 3.292 | |
   | `spine_middle`, `spine_lower` | 3.269 | |
   | `neck` | 3.240 | |
   | `head` | 3.119 | |
   | `upper_leg` | 0.123 | 2.421 |
   | `leg` | 0.255 | 2.070 |

9. **The glTF-to-Blender conversion for a whole orientation is pinned by a
   known answer.** Both sides of the clip rules are read in Blender Z-up world
   space, and the conversion that gets our output there had no test of its
   own: the cross-rig fixture put the same function on both sides, so it
   canceled and any conversion at all passed. It is pinned against the vector
   conversion instead, which carries hand-written numbers: for a composed
   rotation and each of a bone's own axes, `gltf_to_blender_rotation(q) @ e`
   equals `gltf_to_blender(q @ e)`. The fixture builds its vendor orientations
   from converted axis vectors now, so it cannot cancel either.

### Corrections T7 made to this document

1. **`run` does not travel, and the reading that says so is where the hips end
   up rather than how far they wandered.** The row asks for a measurement
   because "a Meshy library clip is likely in place". Measured on the
   committed `run.glb`: its hips end **0.0000 m** from where they started, and
   they wander **0.0276 m** horizontally on the way. The excursion is 38
   percent past the 0.02 m threshold, so the two readings give opposite
   answers on the one clip the row asks about.

   `source.traveling` therefore reads the **endpoint**, horizontally, and the
   field's own doc comment says so, so the declaration and the measurement
   cannot mean two different things. An in-place vendor export pins the root
   exactly, so both readings separate it from a traveling one and only the
   endpoint separates it from a run cycle's own hip sway. Vertical is excluded
   because fact 5 already names the bob as animation rather than travel, and a
   run's is 0.043 m, twice this whole threshold.

   **The excursion is a rule of its own, `source.wander`, beside it.** This is
   the only boundary that can read it: `strip_root_motion` pins every key's
   horizontal position onto the first key's, so what `clip.root_travel` sees
   at the bake is a residual of about 2 nanometers whatever the vendor sent,
   and `--keep-root-motion` reports both bake rules as `skipped`. So a clip
   that slid out and came back would have been measured by nothing. It records
   rather than gates, at a ceiling of 1000 m: a run cycle wanders 0.0276 and a
   strafe 2.3117, 84 times further, and no one threshold reads both.

   Recorded, hips first frame to last and hips at every frame: `idle` 0.0000 m
   wandering **0.0112**, `run` 0.0000 wandering **0.0276**, `walk_back` 1.2712
   wandering **1.2712**, `strafe_left.fbx` **2.3117** wandering **2.3117**.
   That 2.3117 is the number the Test Plan already carried and the one T5's
   correction 15 used for the femur ratio. So `travels` is `false`, `false`,
   `true`, `true`.

2. **The up axis is `clip.root_bob`, a rule and a limit of its own.** The
   acceptance asks for "under 2 cm on all three axes". The two horizontal ones
   read **1.8e-9 to 3.7e-9 m** on the three committed clips once the rewritten
   strip has pinned them, which is the `f32` an F-curve stores. The third
   cannot: `strip_root_motion` keeps the vertical channel, so what is left
   there is the bob, and it measures **0.0089 m on `idle`, 0.0377 on
   `strafe_left`, 0.0391 on `walk_back` and 0.0535 on `run`**. Gating that at
   0.02 m would reject every correct clip, and the only way to pass would be
   to flatten a jump onto the ground, which the same design forbids two
   paragraphs earlier.

   Reporting it as `skipped` would be worse, because no declaration switches
   it off: the axis the bake keeps is where the defect this task fixes was
   largest. Measured on the freshly refitted `strafe_left`, the strip this
   replaces leaves **0.0428 m on X**, 0.0170 on Y and **0.2911 on Z**, against
   the 0.315 the Test Plan recorded for the pre-rename output. So the up axis
   is gated at **0.15 m**, which is 2.8x the worst real bob and 1.9x under
   that sink. `--keep-root-motion` is the one thing that switches either rule
   off, and it reports `skipped` on all three axes.

3. **`source.traveling` is two rule ids.** "The threshold both ways, `ge` when
   `travels` and `le` when not" is one id with two comparisons, and T1's
   registry binds one comparison to one id: `Report::off_registry` refuses a
   finding whose comparison is not the rule's, which is the only thing
   standing between a hand-written Python finding and a limit nobody
   published. So the declaration picks the rule instead. A clip that travels
   is read by `source.traveling` while `source.in_place` reports `skipped`,
   and a clip that does not is read the other way around. Both are printed by
   `--list-rules`, both carry `[profile.source] travel_meters`, every clip
   reports on both, and both negatives still fail.

4. **`clip.fps_grid.range` cannot read the scene's render range.** The Logic
   sketch compares `(scene.frame_start, scene.frame_end)` against the action's
   rounded range. Measured: the glTF importer sets no render range at all, so
   on a correct refit of `run.glb` that reads Blender's default **1 to 250
   against an action spanning 0 to 20** and the rule fires on every correct
   clip. Setting the range from the action first would make it self-fulfilling.

   What it measures instead is the count T5's correction 16 already named:
   the frames the retarget samples against the source's own key times. A 30
   fps clip read at 24 spans 0.8 to 16.8, rounds to 1 to 17, and **drops four
   of its 21 frames**; that 4 is the reading. It reads a gap the other way
   too, which is also worth knowing: a source that does not key every frame is
   one the retarget samples between its keys.

5. **The fps rate is read from the key spacing, not from the scene.** "The
   file's rate" is only the scene's rate for an FBX, where the importer sets
   it from the file and every key is whole. A glTF stores key times in seconds
   and the importer converts them at whatever rate the scene is on, so nothing
   about the scene says what the file was authored at. `scene_fps` divided by
   the median key spacing says it for both: a 30 fps clip read at 24 lands its
   keys 0.8 frames apart and reads 30 either way. The reading is rounded to a
   whole rate before the comparison, because it is a ratio of two floats and
   every rate the library declares is an integer; a genuinely fractional rate
   such as 29.97 is refused one step later by `clip.source_motion`.

6. **`source.posture` reads joint directions, not bone axes.** Fact 17 records
   `strafe_left` as "authored with the head 34 to 37 deg forward". Measured on
   that file, the head **bone's** own axis sits **1.027 degrees** off vertical
   while the direction from the `neck` joint to the `head` joint leans
   **34.927 to 35.851**. The bone axis would have missed the hunch entirely,
   which is the blind spot correction 6 names: nothing in the published set
   sees a joint chain. So all four readings are joint directions, and
   `source.child_axis` beside them is what explains the difference.

7. **The three recording rules read against a ceiling, and that is
   deliberate.** An `info`-only rule still needs a limit, because
   `Rule::measured` reads the comparison to decide the severity and
   `off_registry` compares the limit against the published one. So
   `source.child_axis` and `source.posture` sit at half a turn, the largest
   angle two directions can be apart, and `source.wander` at 1000 m, further
   than any clip moves its hips. The comparison always holds and every reading
   is information. The alternative was a severity a caller sets by hand, which
   is exactly what the registry exists to stop.

8. **The three Mixamo clips are three FBX files on disk, and two of them are
   library entries.** `art/staging/downloads/` holds `strafe_left.fbx`,
   `strafe_right.fbx` and `walk_back.fbx`, all three Mixamo exports. Only the
   first two are declared in `library.ron`, as
   `Mixamo(product_id: "c9c97b90-…")` and `Mixamo(product_id: "c9c96f9e-…")`.
   `walk_back` stays `Meshy(action_id: 544)`, because T5's correction 10
   counts it among the three committed Meshy clips that the rename invalidated
   and T15 regenerates it; its FBX is a measurement subject only. So the row's
   "true for the three Mixamo clips" is `travels: true` on two library entries
   and one file nothing declares.

9. **`loop_mismatch` and `report_loop` were already gone.** T5 deleted both,
   with the 14 tests its own correction 13 lists. The deletions table now says
   so rather than asking T7 to delete them twice.

10. **The key name `inplace` is unconfirmed, and `source.traveling` is the
    real guard.** Nothing in this task may touch the network, and the
    downloaded FBX carries no export parameters in its metadata: its
    `SceneInfo` names Mixamo, the exporter version and the source skin, and
    nothing else. The only `gms_hash` in this repository is the one
    `test_mixamo.rs` writes by hand, so it is evidence of what we believe and
    not of what the provider sends. The key name stays unconfirmed until a
    live fetch runs. `export_body` sets it to `false` explicitly rather than
    echoing whatever the product call returned, and a unit test pins that the
    request carries the flag with that value whichever way the product had it;
    if the name turns out to be wrong, that request is a no-op and
    `source.traveling` refuses the in-place export it produced, by name, at
    0.0000 m against 0.02. The three FBX files already on disk are traveling
    exports, at 2.3117 m of hip travel, so the provider's default agreed with
    the value now stated; what changes is that it is now stated.

11. **Every published limit reaches Blender on argv.** A script that named its
    own limit would hold a second copy of a number `--list-rules` prints, and
    a `[profile]` reader in Python would be a second copy of the validation.
    So `stages.rs` passes `--limit RULE=NUMBER` for every rule a script
    reports, read off the rule list itself, and `findings.Rule` is the Python
    mirror of `check/mod.rs`'s `Rule`: a script builds its Findings through it
    and never decides a severity. That replaced the hand-rolled `finding()` in
    `retarget_animation.py`, which had been deciding severity in code no test
    could reach.

12. **`clip.fps_grid` is measured at two sites, and neither can be dropped.**
    The retarget reads the action it imported, which is the only place an
    off-grid **import** is visible at all: it samples whole frames, so what it
    writes out sits on the grid whatever it read. `check/clip.rs` reads the
    delivered file's own key times, which is where a resampling **export**
    would show and which is the half that runs in CI.

13. **A bake subject names the clip as well as the axis.** One bake
    invocation carries every animation the spec lists, so three clips times
    three axes share one report and `x` alone would be ambiguous. The subject
    is `run x`, and the bake now reads its own report: it was asserting the
    report was absent, which was the tripwire T7 was meant to trip. It also
    refuses a report that left one of those subjects out, so deleting the
    measurement is a failing stage rather than a quiet pass.

14. **`walk_back.glb` is `clip.loop`'s `[art]` negative, and it was already
    documented in prose.** `library.ron` has carried "this clip does not
    return to its start pose, so it hitches once a loop" since it was written.
    Measured: **6.910 degrees** on `LeftForeArm` and nine of its 24 bones past
    2.0, against `run` at 0.000 and `idle` at 0.487. A comment became a
    number, which is the whole point of the rule.

15. **The fetch report is named `fetch.<clip>.<attempt>.json`.** The audition
    paragraph and T14's acceptance both said `fetch.1.json`, which `Artifacts`
    cannot write: one stage runs once per clip, so the item is in the name and
    a second clip would otherwise overwrite the first.

16. **The three committed Meshy clips are 24 fps.** Measured from the key
    spacing in each committed GLB, which is 1/24 s to within the `f32` the
    accessor stores: `idle` 47 keys over 1.916667 s, `run` 21 over 0.833333,
    `walk_back` 23 over 0.916667. None of the three sprite rates changes: they
    stay 8, 24 and 20, which is what makes `source_fps` a field of its own.

17. **`off_registry` has to read an undefined measurement as agreeing.**
    `Rule::undefined` writes `unit="undefined measurements"`, a limit of 0 and
    an `eq`, on both sides, because an angle that does not exist is not 180
    degrees. Read literally, the registry then names that finding back as four
    disagreements at once, and `check_source` aborts the fetch with "reported
    findings the rule list does not carry" instead of the defect: a vendor
    file with two coincident joints, or a clip with one key, would say the
    wrong thing. So `Report::disagreement` recognizes that exact shape,
    severity included, and still holds it to the rule's own `measured_on`. A
    finding claiming to be undefined on another rule's space is named back as
    before.

### Corrections T8 made to this document

1. **The floor is not zero, and snapping to zero would bury the character.**
   The Logic sketch says `lift = -min(world_z(toe))` and the T8 row says "snap
   the lowest foot frame to Z equals 0". Both put the toe joint at zero.
   Measured on the committed rig: `LeftToeBase` rests **0.031081 m** above
   zero and `RightToeBase` 0.030723, because on this
   skeleton the toe joint is the **ball of the foot** and there is no toe-tip
   joint. Zero is where the sole is: `model.glb`'s mesh spans exactly 0 to
   1.700000 m in world space, so the rest pose is already standing on the
   ground.

   The datum is therefore the rig's own rest pose: `floor` is the lower of the
   two resting toes and the lift puts the clip's lowest toe frame there. The
   evidence that this is the right datum is the committed art. `run.glb` and
   `walk_back.glb` reach **0.029066** and **0.029429 m** at their lowest,
   which is 1.7 and 1.3 mm under that floor and inside the 5 mm limit;
   against zero they would read 29 mm and be rejected, and the fix would be to
   sink every clip three centimeters into the tile.

   The rule earns its place on the same art. `idle.glb` was fitted before any
   snap existed and its lowest toe hangs **0.0773 m** above the datum, which
   is the character standing in the air; a refit through the new retarget
   brings it to 9.3e-9 m. That is the rule's `[art]` negative and a
   `cargo art fetch idle --force` away from being fixed.

   **What this datum is not.** It is not "the sole touches the floor". It is
   "the toe joint returns to its rest height and never goes below it", and the
   two coincide only while the foot's orientation at the clip's lowest frame
   matches rest. That holds for today's plantigrade clips, and it fails for a
   pointed toe at the lowest frame: a kick, a jump landing or a sprint
   push-off would be lifted onto its toe tip and float, and a heel strike
   would sink its heel. The rule's message therefore says what it measures,
   "the distance from the rest height the snap aims at", rather than claiming
   a floor. The true sole datum arrives with T9's
   `clip.foot_contact.penetration`, which reads the mesh rather than a joint.

2. **`clip.floor_snap` is measured at two sites, like `clip.fps_grid`.** The
   retarget reads the pose it evaluated, which is the only place the lift
   itself is visible, and `check/gltf_clip.rs` reads the delivered file's own
   joint transforms per frame, which is where an export that resampled the
   root would show. Both are Blender Z-up world space, because the file's
   rest pose comes out of the same graph: the exporter writes the armature at
   rest already, which `clip.twist` requires and `test_blender.rs` enforces.
   The Rust half is what gives the rule a negative control in CI, per rule
   four, and its calibration pair sits either side of the published 5 mm at
   4 mm and 6 mm.

   `clip.stride` and `clip.stride_ratio` are read at both sites for the same
   reason, and the Rust half adds one thing the Python half cannot: it
   re-derives **our own** stride segment off the rig GLB's rest joints rather
   than reading the length the retarget wrote, so a wrong `segment_length`
   is caught. It does not catch a wrong choice of `stride_segment` roles,
   because that choice sits on both sides of the ratio. The source's own
   travel and femur cannot be re-derived at all, because the vendor file is
   an FBX, so both ride in the sidecar beside its rotations and
   `check/motion.rs` refuses a travel below zero or a segment of no length.

3. **The travel band is `clip.stride`, and the femur ratio is
   `clip.stride_ratio` beside it.** The row asks for a rule rather than a
   printed line and names neither. `clip.stride` reads the fit's own root
   travel against the source's sized by the femur, as a **relative**
   difference so 2 percent means the same on a 0.4 m shuffle and a 2.3 m
   strafe. `clip.stride_ratio` records the ratio itself against a ceiling of
   100, the way `source.wander` records at 1000 m, because a Mixamo rig and a
   refit of our own clip are both right and no threshold reads both.

   Both are calibrated on a real local fit of the three FBX files on disk,
   which reproduces T5's hand measurement. Every reading is written once, in
   the Test Plan row, so no comment anywhere can drift off it.

4. **Which roles stand on the floor is skeleton data, like the stride
   segment.** T5's correction 15 put `stride_segment` in the skeleton file for
   the same reason, and a quadruped's floor is four joints, not two. So
   `humanoid.toml` gains `ground_roles`, refused at load time if it names
   something that is not a role, names one twice, or is empty. Both readers
   validate it: `skeleton.py` for the retarget and `check/aim.rs` for the file
   side, which is the reader that already owns the role tables.

5. **The bake's own sizing is now a refusal, and its band is 1e-4 rather than
   1e-6.** `size_to_character` scaled every clip's translation by a
   **total-height** ratio at bake time, which is the metric T5 replaced and a
   second scaling on top of the femur one. Measured on the three committed
   clips against the committed character: **1.2270508611411657e-06**, the
   same on all three, which is the `f32` a GLB stores a joint position in and
   not a real difference. So the bake refuses a clip that is not on this body
   instead of quietly rescaling it, and the band is 1e-4: 81x over that noise
   and 1,000x under the 12 percent a Mixamo femur differs by. A band of 1e-6
   would reject all three committed clips.

6. **The rest heights the bake compares are armature units, not meters.** They
   come from `rest_points`, which reads `head_local` on purpose so an object
   scale is not counted twice, so the committed pair reads 166.5169 against
   166.5167 rather than 1.665. The refusal says "in armature units" rather
   than "m".

7. **What `clip.stride` proves, and what it cannot.** Its two sides are
   algebraically identical up to float noise: `write_keys` copies the source's
   own root world position, and `scale_translation` multiplies every location
   key by `ratio`, so the fit's travel *is* the source's travel times `ratio`
   by construction. That is why every reading in the Test Plan row is float
   noise rather than anything a body would produce. The rule is still worth
   its place, because it gates the **write path**: with `scale_translation`
   removed the same fit reads 13.4409 percent, and the rule also catches a
   key dropped, resampled or rescaled between `transfer.py` and the delivered
   GLB.

   It cannot catch a wrong choice of `stride_segment` roles. Both segments
   are measured across the same two roles, so naming the tibia or the whole
   leg changes `ratio` and moves both sides of the comparison together. What
   guards that choice is `clip.stride_ratio` recording the number and a human
   reading it, plus T9's `clip.foot_contact.skate`, which reads a planted
   foot against the ground and does not go through `ratio` at all.

### Corrections T9 made to this document

1. **The toe joint is not the point that touches the ground, so the contact
   threshold cannot be read on it.** The Logic block asks for
   `world_z(toe) < 0.03 * scale`. Measured on the committed rig
   `art/skeletons/humanoid.glb`, which is the datum every number in this
   correction is read on: its joints span **1.665165 m**, so `scale` is
   0.925092 and the ceiling is **0.027753 m**, while `LeftToeBase` rests at
   **0.031081 m** and `RightToeBase` at 0.030723, the pair T8's correction 1
   records. The floor snap puts a clip's lowest toe frame
   exactly on the lower of those and never below it, so no frame of any clip
   can reach that ceiling: read on the joint, every clip on this skeleton
   reports zero plants and `clip.foot_contact.plants` fails all of them.

   The threshold is therefore read on the **sole point under the toe**, which
   rests at zero by construction. `scale` comes from the rig's own joint span
   rather than from a character height, because the retarget is handed a
   skeleton and never a spec.

   A delivered clip stores the same joints as `f32`, so read there the three
   are 1.665169, 0.031082 and 0.030719 m. That is 4 micrometers of export
   rounding and correction 10 says what the two sites do about it.

2. **How the sole is derived from joints, on a clip that carries no mesh.**
   The rig's rest pose stands on the floor: `model.glb` spans exactly 0 to
   1.700000 m in world space. So the point directly below a joint at Z = 0 in
   the **rest** pose is a point of the sole. It is held in that joint's own
   rest frame, `sole = rest^-1 @ (x_rest, y_rest, 0)`, and carried rigidly
   afterwards, `world = pose @ sole`. Two per foot, under the ankle and under
   the toe, because those are the only two points of a foot a mesh-less clip
   can locate.

   The evidence that this is the right model is the vendor file. On
   `art/staging/downloads/strafe_left.fbx`, read through the same math
   against Mixamo's own rest pose, both sole points read **0.0000 m at every
   planted frame** of both feet, and 0.0038 m at worst over the whole clip.
   This is the true sole datum T8's correction 1 said would arrive here, and
   it reads joints rather than a mesh.

3. **The right foot of every cross-rig fit never plants, and the cause is
   this rig's own hips.** Measured on a fresh local fit of the three FBX
   files, the sole point under the right toe, which is the one contact is read
   on, never gets nearer the floor than **0.1831 m** (`strafe_left`), 0.2042
   (`strafe_right`) or 0.1888 (`walk_back`), while the
   source's own right toe stands on its floor at frames 1 to 4 and 20 to 21.
   `clip.swing` reads 4e-5 degrees on the same fit, so every bone points where
   the source's does and this is a **position** defect that no published rule
   could see.

   The cause is measured: `reference_pose` turns our `Hips` bone **97.6
   degrees** to reach the aim table, which `rig.aim_table` already reports at
   97.8 against a band of 75, and every joint under the hips is carried by
   that turn. At frame 3 of `strafe_left` the fit's `Hips` to `RightUpLeg`
   offset is (-0.0072, -0.0692, +0.1050) m against a rest offset of
   (-0.0875, +0.0019, -0.0906). The datum there is that offset and not a
   height: its Z component rises **0.1956 m** between the two, and the left
   socket's rises 0.0157, and the floor snap then lowers the whole clip until
   the left foot lands. The same fit's `Hips` to `Spine` direction sits 97
   degrees off the source's.

   That pair of offsets checks itself: both are **0.1260 m** long and they sit
   **124.6 degrees** apart, so the socket is turned about the hips rather than
   carried away from them.

   The world-height view of the same fault, for a reader who wants heights
   rather than offsets: at frame 3 `RightUpLeg` stands at z **0.9937 m**,
   0.1241 above its own rest 0.8696, while `LeftUpLeg` stands at 0.8154,
   0.0559 below its rest 0.8712. The pelvis is therefore tilted **0.1800 m**
   at that frame, and between 0.1795 and 0.1831 at every one of the clip's 21
   frames. `RightToeBase` itself never gets below **0.2136 m**.

   **This is not something planting can fix.** An XY lock cannot lower a foot
   18 cm, and a solve that did would be hiding a rig that already fails its
   own gate. It is fixed by regenerating the rig, which is T15's. Until then
   `cargo art fetch strafe_left` and `strafe_right` stop at the retarget with
   this rule naming the foot. No committed art goes through the retarget: the
   survivor's three clips are Meshy's and arrive on this rig already.

4. **The left foot of every cross-rig fit digs 2 cm, and the cause is that
   the two rigs' feet are different shapes.** `clip.foot_contact.penetration`
   reads **0.0203 m** on `strafe_left` and `strafe_right` and 0.0200 on
   `walk_back`, always on the sole under the ankle, and the three readings
   agree to a tenth of a millimeter because none of them is a property of the
   clip. Measured: our rig's left foot rests **39.46 degrees** below
   horizontal from ankle to toe and Mixamo's rests **27.29**. The transfer
   matches bone directions exactly, so at the planted frame the fit's foot
   points 27.28 degrees below horizontal, which is the source's rest angle
   and **12.18 degrees flatter than our own rest**. Our own rest is the pose
   whose sole is on the floor, so the sole tips that far under it.

   A refit of `run.glb` onto its own rig, where no rest pose differs, clears
   the floor by 0.0074 and 0.0005 m. So the rule is calibrated on the vendor
   file and on the same-rig refit, and the three cross-rig readings are its
   `[art]` negative. Correcting the foot's pitch during stance is a third
   bone of solve that the T9 row does not ask for, and the honest reading is
   the one recorded here.

   **The sole model has a limit of its own, and this reading sits on it.**
   "The joint carried down to zero at rest" is exact for the ball, because the
   toe joint really is above the contact patch. It is a fiction for the heel:
   on a plantigrade foot the ankle is not above the heel, so the point under
   the ankle is a point of the sole only in the rest pitch. Pitch the foot
   flatter than rest and that point reports penetration even where the real
   sole clears, which is what the 0.0203 m above is. The vendor file's
   0.0000 m validation therefore proves the model on a rig whose rest pitch is
   preserved and nowhere else. T15's regenerated rig removes the cause; until
   then, read a penetration on the ankle point as a pitch difference and not
   as geometry through the floor.

5. **`travels: false` switches both stance rules off, the way it already
   switches `clip.stride` off.** An in-place cycle's ground moves under it, so
   its feet must slide: measured on the refit of `run.glb`, its soles cross
   the floor at **3 to 5 m/s** while the contact threshold is 0.278. Nothing
   in the clip declares the body speed those readings would have to be taken
   against, so the measurement does not exist rather than failing. The Test
   Plan's `[synth]` in-place negative for `plants` is therefore a `skipped`,
   and the real negative is a clip declared traveling whose feet ride along
   with its root.

   **Open, owned by T15 or later.** A skip leaves `run.glb` with no contact
   reading at all, which is a hole rather than an answer. The way to close it
   is to let an in-place cycle declare its body speed implicitly, as the mean
   horizontal velocity of the stance foot, which is the treadmill speed:
   `plants` is then measured against speed relative to that, and `skate`
   becomes deviation from a constant velocity rather than from a fixed point.
   T9 does not build it, and the two rules stay `skipped` until something
   does.

6. **The two sites do not read the same runs, and that is the point.** The
   retarget detects contact on the path before the lock and reports the drift
   the lock left; `check/foot.rs` detects it again on the delivered file, so
   it sees the ramps as well. Measured, they agree on `strafe_left` and
   `walk_back` frame for frame and differ by one frame on `strafe_right`,
   where the delivered file's run starts one frame earlier and that frame
   carries **0.0021 m** of drift the lock never held. Both readings are
   inside the published 2.5 cm and both are reported.

7. **The first frame is read against the next one.** It has no previous
   frame, and reading it against itself makes it still by construction, so a
   foot that starts on the ground and leaves at once plants for exactly one
   frame. The step is therefore taken to the neighboring frame, previous
   where there is one and next at the start, and the synthetic cross-rig pair
   pins it at both sites.

8. **The rule list is 49.** T8's 46 plus these three. `clip.foot_contact.*`
   lives in `check/foot.rs` rather than in `check/clip.rs`, the way
   `rig.aim_table` lives in `check/aim.rs`: one module owns the rules, the
   math and the findings of one concern. `clip.rs` lists all three in
   `RETARGET_RULES` and `FILE_RULES`, which is where `stages.rs` reads them
   and what `refuse_unread_rules` holds the report to.

9. **A skip is not a defect.** `Rule::skipped` files a reading of 0.0, so a
   `travels: false` clip's two skipped `clip.foot_contact.plants` findings sit
   at 0 against a limit of 1 and any count of "findings outside their limit"
   picks them up. `stages.rs` counts `Severity::Error` only, which is what
   `has_errors` gates on, so the number in a refusal is a number a reader can
   find in the report. The same message names the distinct rules that failed,
   sorted, because a count alone says nothing about what to open.

10. **Both sites scale the contact thresholds by the same file's joints.**
    `retarget_animation.py::rig_height` spans the armature's bones inside
    Blender and `check/gltf_clip.rs::joint_span` spans the rig file's skin
    joints, and that is one set: glTF has no bone outside a skin, and
    Blender's glTF importer creates exactly one bone per skin joint. The set
    is not every node either, because the committed rig also carries an
    `Armature` node and a `skin_carrier` node, and spanning those reads
    **1.695888 m** against the joints' 1.665165. The file-side rules read the
    span off the **rig** rather than off the delivered clip: the clip's own
    copy of the same joints reads 1.665169 m, and taking it would leave the
    two sites 7e-8 m apart on the ceiling for no reason.

### Corrections T10 made to this document

Every number here is measured on a **stand-in**, and that is the first
correction. `bare.glb` still has not been downloaded, because the only Meshy
key on this machine answers 401, so T10 lifted the survivor's mesh out of the
committed `model.glb` instead: imported, the armature modifier applied at its
rest position, the parent cleared with the world transform kept, and exported
unskinned to `art/staging/survivor/bare.glb`. That file is a faithful stand-in
and the readings prove it: welded it measures holes **171**, non-manifold
**8**, pieces **7**, mirror **3.021047 percent** and 54,864 triangles, which
is `model.glb` exactly, and self-intersections **859** against 861, two fewer
because the export re-split 16 vertices. It carries the same shape as the real
family too: one `char1` node at a 0.01 scale over vertex coordinates 100x
larger, one image, one UV layer.

1. **The fixer makes two of the four defect classes worse, and
   `cleanup_effective` cannot count either.** Measured end to end on the
   stand-in, each step in the order the Logic section states:

   | After | holes | non-manifold | pieces | crossing faces | mirror | height error |
   |---|---|---|---|---|---|---|
   | nothing, as downloaded | 171 | 8 | 7 | 859 | 3.021 | 0.000 |
   | weld, then drop debris | 111 | 8 | 1 | 848 | 3.021 | 0.000 |
   | fill holes | 40 | 11 | 1 | 905 | 3.021 | 0.000 |
   | symmetrize | **73** | **12** | **2** | **1026** | **0.000** | **0.349** |

   The third row is also what a `symmetry: false` character gets: a real run
   with the flag off writes exactly it, and leaves the mirror distance at
   3.021 rather than driving it to zero.

   The height move is not an artifact. Measured before the mirror, the +X
   half of the body is **1.694069 m** tall and the -X half **1.699935 m**,
   and `direction="X"` keeps the +X one, so the mirrored body is 5.93 mm
   shorter than the 1.700 m it arrived at. That is the kept half's own
   height, 0.349 percent, well inside `mesh.world_size`'s 5 percent band.

   Filling a hole closes it with a face that can meet two others, which is
   what `non_manifold_post` was already published for: 8 to 11. **Mirroring
   is the one the design did not see.** It copies the geometry of the half it
   keeps, so the crossings inside that half are duplicated: 848 to 1026, plus
   33 boundary edges and a second piece where the mirrored surface meets the
   original. So a symmetrized mesh cannot be expected to have fewer crossing
   faces, and `mesh.cleanup_effective` counts **holes and pieces only**. On
   the stand-in that is 178 to 75.

   Self-intersections keep their own gate, `mesh.self_intersect`, and the
   cleaned stand-in reads **1026 against its provisional ceiling of 1000**.
   The file rules run on the file that gets rigged, so that is a failing gate
   today: `cargo art check` exits non-zero on the survivor and `rig()` refuses
   to spend 5 credits on the mesh. There are two ways out and neither is
   silent. Recalibrate on the real `bare.glb` cleaned, which is what the row
   was marked for from T3 on. Or raise the ceiling, with the measured
   headroom written beside it the way every other row states its own. This is
   also why the row cannot be recalibrated on `bare.glb` alone: it has to
   cover what the fixer leaves.

2. **One reading over the classes, not one per class.** The design's prose
   says "holes, islands and self-intersections strictly decreasing", which as
   a rule per class refuses a fixer that was handed a class with nothing to
   remove: 0 crossing faces in and 0 out reads 0 lt 0 and fails. That is a
   gate failing on good art, and the synthetic fixture hit it on the first
   run. So the rule sums the classes it counts, names each pair in the
   message, and the stub the design asks it to reject still reads the same
   number it was given. The one input it cannot pass is a mesh with no holes
   and one piece, which is a mesh no generator this pipeline uses has
   produced and which needs no fixer at all.

3. **`mesh.cleanup_effective` publishes no limit, and the registry had no way
   to say so.** Its limit is the subject's own count before the fixer ran, so
   `[profile]` cannot state it. The rule answers `NaN` from its limit
   function, `Rule::publishes_a_limit` is how anything asks, `Rule::against`
   builds its findings, and `--list-rules` prints the word `before` where
   every other rule prints a number. `Report::off_registry` then holds those
   findings to the comparison, the unit, the space and the severity, and to
   everything but the one number nobody published. `Rule::measured` on that
   rule would file the NaN, which a report refuses, so the misuse is loud.

4. **The importer's `Icosphere` needs an armature, and sits in a collection
   the exporter ignores.** T5's correction 14 owns this: the object is
   Blender's glTF importer and not the art. What T10 adds is where it goes
   and when, because the fixer imports and exports: the collection is
   `glTF_not_exported`, Blender's own exporter skips it, and importing the
   unskinned stand-in adds no such object at all, because it carries no
   armature.

5. **Every hole is filled, and the alternative is measured.**
   `bmesh.ops.holes_fill` defaults to `sides=4`, which fills only holes of up
   to four edges. On the stand-in that default takes holes 111 to 96 for 4
   extra crossing faces, where `sides=0` takes them to 40 for 57. The fixer
   fills everything, because closing the surface is what rigging needs, and
   both readings are here so the choice is a measurement rather than a
   default nobody looked at.

6. **The fixer measures nothing, and the image and UV layer counts it prints
   are a log line.** Rule three is that the fixer never reports on its own
   work, so `mesh_clean.py` writes no report at all and the runner refuses one
   if it ever does. What proves the texture survived is `mesh.texture` and
   `mesh.uv` on `clean.glb`, in Rust: measured, in
   `art/staging/reports/cleaned.survivor.1.json`, the cleaned stand-in reports
   0 primitives with no base color image and 0 texture coordinates outside
   the tile, and the fixer's own `key=value` line says `images=1 uv_layers=1`.

7. **The mirror flag reaches three rules and the fixer, and the cleanup flag
   reaches two rules.** `symmetry: false` puts `--symmetry false` on Blender's
   argv and reports `mesh.mirror`, `rig.mirror_length` and
   `rig.mirror_direction` as `skipped`, each on every subject it owns, with
   one sentence both sides share. `cleanup: false` runs no Blender at all and
   reports `mesh.non_manifold_post` and `mesh.cleanup_effective` as `skipped`,
   with the pre-cleanup set still applying to the mesh as it arrived. All four
   combinations are pinned through `cargo art check`, and the negative control
   of every silenced rule still runs with the flag on.

8. **The rule list is 51, and the `mesh.*` family has three sets.** T9's 49
   plus the two post-cleanup ones. `FILE_RULES` is the thirteen read off one
   file, `CLEANUP_RULES` is the two read off the pair, and `RULES` is what
   `--list-rules` prints. `CLEANED_RULES` is the third: eleven of the
   thirteen, because two cannot honestly be read on a file the fixer wrote.
   `mesh.non_manifold`'s ceiling is calibrated before the fixer and filling a
   hole raises that count on purpose, so after the fixer it is
   `mesh.non_manifold_post`'s, and one count against two limits is one of
   them wrong. `mesh.printability` asks Meshy about a model task, and a local
   file has none, so it could only ever warn that nobody answered.

9. **The gate runs before the money, inside the rig stage, and writes three
   reports.** `clean_mesh` measures the mesh as it arrived, runs the fixer,
   measures the file it wrote and then the pair, writes
   `mesh.<char>.1.json`, `cleaned.<char>.1.json` and `cleanup.<char>.1.json`,
   and refuses to go on if any of them holds an error. One report per rule
   set, because `cargo art check` writes the same three by hand and a stage
   name carrying two shapes would have one producer overwrite the other's
   file. Rigging is then handed the file by value: 4.19 MB of GLB is
   **5,592,166 characters** of `data:model/gltf-binary;base64,...`, and
   `input_task_id` is absent, because it wins if both are sent and the
   cleanup would be discarded. Meshy states no body size limit, so that
   number is what the first real call has to check.

10. **The mirror runs through `bmesh`, and `POSITIVE_X` is not a word it
    knows.** The Logic block calls `bpy.ops.mesh.symmetrize`, which needs an
    edit-mode context; every other step here is a `bmesh` operation, so the
    mirror is one too. `bmesh.ops.symmetrize` takes `direction` out of
    `['-X', '-Y', '-Z', 'X', 'Y', 'Z']` and rejects `+X`, and measured on a
    box with a spike out at +X only, `direction="X"` keeps the spike and
    copies it to -X while `-X` deletes it. So `"X"` is the operator's
    `POSITIVE_X`, proved rather than assumed, and `dist` is its `threshold`.

11. **The mesh goes by value whether or not the fixer ran.** The Test Plan
    asked for `input_task_id` when `cleanup` is off. It is never sent: with
    the flag off the request carries `bare.glb`, and the file that was
    measured is then the file that gets rigged, on one code path instead of
    two. `input_task_id` wins if both are sent, so having it in the body at
    all is the failure this is avoiding.

12. **Still blocked, and where the steps are.** No Meshy call happened in
    T10: `MESHY_API_KEY` is unset and the stored key answers 401. So
    `bare.glb` is a stand-in, `non_manifold_post` is provisional, and the
    5-credit rigging call by data URI is unproven. The exact sequence a key
    unblocks is in `crates/xtask-art/README.md`, under "Still waiting on a
    Meshy key": fetch the paid model task's own GLB for 0 credits, run
    `cargo art check`, replace every provisional row with what it read,
    re-run the rig stage for the 5 credits, and measure `clean.glb`.

13. **The mirror's cost is inherent: three ways out measured, none adopted.**
    The extra 33 boundary edges, the second piece and the 121 extra crossings
    of correction 1 look like an unmerged seam at X = 0, so every way of
    removing one was measured on the stand-in, through the fixer's own code
    and read by the Rust gates off each exported step:

    | Ordering, at the last step | holes | non-manifold | pieces | crossings | mirror | height |
    |---|---|---|---|---|---|---|
    | fill, then mirror: the design's | **73** | **12** | **2** | **1026** | 0.000 | 0.349 |
    | mirror, then fill | 62 | 14 | 2 | 1052 | 0.000 | 0.349 |
    | the design's, then a weld of the seam alone at 1 mm | 71 | 11 | 2 | 1006 | 0.000 | 0.349 |
    | the same, and fill a second time | 60 | 13 | 2 | 1012 | 0.000 | 0.349 |

    Mirroring first is worse on two counts and better on one, so the order
    stands. The seam weld merges 14 vertices and recovers 2 of the 33
    boundary edges, 1 of the 4 non-manifold edges and 20 of the 121
    crossings: 6 percent of the cost, for a step and a test, and no gate
    changes its verdict. Filling again after it trades those non-manifold
    edges back. So the seam is only part of the cost, the rest is inherent to
    copying a half, and the fixer keeps the order the Logic block states.

    The mirror threshold was swept on that order, and nothing here holds
    every gate either:

    | `symmetrize_meters` | holes | non-manifold | pieces | crossings |
    |---|---|---|---|---|
    | 0.0005 | 60 | 6 | 1 | 1080 |
    | **0.001, published** | 73 | 12 | 2 | 1026 |
    | 0.0015 | 103 | 32 | 2 | 980 |
    | 0.002 | 119 | 46 | 1 | 973 |
    | 0.005 | 274 | 189 | 1 | 1404 |

    0.0005 is better on holes, non-manifold and pieces, including no second
    piece at all, and worse on the one row already over its ceiling. 0.0015
    and 0.002 buy that row by taking `non_manifold_post` past its 20. So the
    published 0.001 stays, and this table is where a recalibration on the
    real `bare.glb` starts rather than a fresh guess.

### Corrections T11 made to this document

1. **The five `concept.*` rules are measured in Rust, not in
   `concept_check.py`.** The T11 row names a Python module and Rule 4 rules
   it out: CI-side measurement is Rust and Python measures only what needs
   `bpy`. A PNG needs none. Fact 16 is the other half: the CI runner is
   `ubuntu-24.04-arm` and nothing there can call `bpy`, so a Python image
   check would have no negative control in CI at all. The Python environment
   also carries no image library, only `bpy` and `mathutils`, while `image`
   0.25 with the PNG feature is already a workspace dependency that
   `preview.rs` decodes with. So the module is `check/concept.rs`, and
   `tools/blender/src/concept_check.py` is not written.

2. **There is no alpha channel to separate the figure with.** All four
   committed views carry exactly one alpha value, 255, so the silhouette is
   found by color: the background color is the median of the image's own
   one-pixel border, and the background is what a flood fill from that border
   reaches within 12 levels on every channel. Filling rather than
   thresholding is what makes an enclosed patch of backdrop, between two
   fingers or under an arm, part of the figure that encloses it.

   The 12 is calibrated rather than picked. At a tolerance of 5 the four
   views read **23.821, 22.922, 14.104 and 13.166** percent of themselves as
   figure, and at 25 they read 23.502, 22.520, 13.886 and 12.947: at most
   **0.402** percentage points of movement across a fivefold change in the
   threshold, so 12 sits in the middle of a wide valley. Only the size of that
   movement is a reading. The direction is not: a looser tolerance can only
   grow the background, so the share falls with it on any art at all, which is
   why `test_concept.rs` asserts the 0.402 and not the slope's sign.

3. **`concept.single_figure` publishes no `[profile]` number, and it is the
   second rule that does not.** Its limit is one figure, and a count of
   figures has nothing to tune, exactly as the `rig.names_standard` family's
   limits table row already states for a count of defects. `[profile.concept]`
   therefore holds four numbers, not five. What is calibrated for this rule is
   the speck floor: 0.05 percent of the image, 786 pixels of 1024 by 1536,
   against real figures of 204,657 to 371,283 pixels and no other piece over
   4. That floor is a constant in `check/concept.rs` with the readings beside
   it, the way `FOOT_SLICE` is in `check/mesh.rs`, because it is a parameter
   of the measurement rather than a published limit.

4. **`concept.mirror` publishes the 99th percentile of the rows.** All three
   statistics were measured, on both views the rule owns and on the Test
   Plan's negative, which stretches the left half of the front view's arm rows
   outward by 15 percent:

   | Reading | `front.png` | `back.png` | the negative | worst view to negative |
   |---|---|---|---|---|
   | mean over the rows | 0.301 | 0.286 | 1.260 | 4.2x |
   | 99th percentile | 1.167 | 1.062 | 6.394 | **5.5x** |
   | worst row | 4.669 | 3.984 | 6.522 | 1.4x |

   The worst row is hair, on art that is symmetrical everywhere it matters, so
   a limit set from 4.669 would have to sit inside a 1.4x gap and would refuse
   the next haircut. The percentile drops that one row and still separates
   widest, wider than the mean, which dilutes a defect covering a fifth of the
   rows. So the published limit is **2.4 percent**, 2x the worst view, with
   the negative 2.7x above it, and the mean and the worst row go in the
   message. `mesh.mirror` publishes a tail statistic too, its worst vertex,
   which this now agrees with in kind.

   **The space is narrower than "one view against its reflection".** Only each
   row's leftmost and rightmost figure pixel is read, so the rule measures the
   silhouette's outline and interior asymmetry reads 0. The published space
   string says exactly that.

5. **`concept.arm_gap` is a share of rows and compares `ge`.** The design
   says "the number of background gaps crossing a horizontal band", and a
   count of two against a limit of two has no headroom to record: every
   correct view reads exactly the number the gate demands. So the reading is
   what share of the band's rows show both gaps, which is a percent with room
   to calibrate: 86.054 and 86.316 against a limit of 75. The 14 percent that
   read nothing are the shoulder rows at the top of the band, where an
   A-posed arm still touches the ribcage on both views, so the shortfall is
   the pose and not a defect.

6. **`concept.arm_gap` and `concept.mirror` own two views, and
   `concept.cross_view` owns six pairs.** A side view shows the arms in front
   of the torso and has no left half to read against a right, so neither rule
   resolves a subject there. That is fewer subjects rather than a skip:
   `Severity::Skipped` stays behind a declaration, and `spec.subject.symmetry`
   is the only one any concept rule reads. The eighteen findings one attempt
   files are 4 background, 4 figure counts, 2 gaps, 2 reflections and 6 pairs.

7. **The concept report is `concept.<char>.<attempt>.json`.** The retry loop's
   own bullet said `concept.<attempt>.json`, which no `Artifacts` name can be:
   the item is in every report stem so one character's run cannot overwrite
   another's. Corrected above.

8. **`cargo art check` runs the same rules under the same stage name.** T10's
   correction 9 is that two report shapes under one stage name have one
   producer overwrite the other's file, so the mirror and the gate share one
   producer, `stages::check_concept`, which measures, writes, and then refuses
   a report that left a rule unread or a subject unreported. `check` calls it
   at attempt 1 and the loop calls it once per attempt.

   `refuse_unreported_subjects` was written for the bake's per-axis subjects
   and now takes the list of subjects a stage owes, because the concept stage
   owes ten of them and the bake owes one per axis of every clip. One
   function, two callers, and the sentence it refuses with is unchanged.

9. **The rule list is 56.** T10's 51 plus the five concept rules, which print
   first because they run first.

10. **A run test needs four concept views that pass the gate.** The end to end
    tests served a 2x2 red PNG as every generated view, which holds no figure
    at all and now fails `concept.single_figure` three times over before the
    model stage is reached. They serve a 128 by 192 synthetic figure instead:
    a flat backdrop, a head, shoulders, a torso, one arm hanging clear on each
    side, and two legs. It reads 0 levels of background spread, 1 figure, 100
    percent of its torso band clear, 0.000 percent of reflection error, and 0
    across every pair because the same image serves all four views.
    `test_concept.rs` holds it to all five rules, so a change to it fails in
    one place rather than in eight end to end tests at once.

11. **The four pause prompts are gone, and with them the only way a run could
    stop on a question that was not about money.** `should_pause`,
    `pause_for_review` and the four tests that exercised them are deleted.
    `--yes` keeps its one meaning, `ConfirmSpend`, and `--retry` keeps its
    own: re-run a stage already recorded as complete. Deciding that a run
    with no terminal attached must stop was the behavior decision 13 replaced
    with a pull request.

12. **`concept.background_flat` reads the region the border fill reached, and
    nothing outside it.** Every pixel it histograms is within 12 levels of the
    border median by construction, so it cannot report a wide spread: a wall
    over a floor reads the wall's own 3 levels and passes. The share the fill
    covered therefore rides in the finding's message, `76.394` percent of the
    frame on the committed front view, so a reading taken on half an image
    says so.

    What refuses the case is another rule. A second backdrop tone the figure
    does not touch is a piece of silhouette of its own, and
    `concept.single_figure` reads 2: measured at 72.412 percent filled with a
    patch over the top-left fifth. A second tone that touches the figure has
    no fixture, so nothing here claims a number for it. The same fill runs
    the other way too: a figure region within 12 levels of the backdrop and
    touching it, a white shirt on a white wall, is eaten and leaves what it
    held as an extra piece. A hood painted the wall's own color over the neck
    rows takes `single_figure` to 2 while the spread stays at 3.

13. **`stages::concept` reuses nothing, and its `force` parameter is gone.**
    The loop's bullet above asked for `force = true` on every attempt, which
    made the parameter's only value `true` and both of its reuse branches
    unreachable. So the branches and the flag are deleted: the stage generates
    four views whenever it runs, and `plan` reading the lock is the only cache.
    Attempt 1 is included, which is the point. A set already on disk is either
    one a previous run left failing, which no retry budget should be spent
    re-measuring, or one this run was told to replace.

    **What that costs** is the one case reuse was for: a run that died after
    three of the four views arrived pays for all four again, about 0.60 USD.
    The T11 success criterion "the test asserts attempt two called `concept`
    with `force = true`" is met by asserting that attempt two generated a
    fresh set, because there is no longer a flag to pass.

14. **The four concept limits are three ceilings and one floor.**
    `arm_gap_rows_percent` compares `ge`, per correction 5, so the profile's
    validation comment naming "every concept ceiling" was wrong about it. All
    four are still refused at zero, and the reason differs: a ceiling of zero
    describes art no generator returns, while a floor of zero is reached by
    every image including a blank one, which is a gate that gates nothing.

15. **A declaration outranks an unreadable file.** An unreadable view is an
    error under every rule that owns it, per the T11 row, except one that a
    declaration already switched off: with `symmetry: false` the two
    `concept.mirror` findings stay `skipped` whatever the files hold. A rule
    nobody asked for cannot fail, and `skipped` is the word that says nobody
    asked, so overwriting it with an error would report a defect against a
    rule that never ran.

### Corrections T13 made to this document

1. **The fingerprint reads files and a process, so `lock::Inputs` exists and
   three functions now return `Result`.** `fingerprint(stage, spec, library)`
   had nowhere to put a repository root or a Blender build. Every caller
   builds one `Inputs` (the root, the spec, the library, the build) and
   `Lock::is_current`, `Lock::record` and `cli::plan` answer `Result`, because
   a missing committed input is an error rather than a hash of nothing. The
   build is a `blender::Build`, a cell that runs `blender --version` at most
   once and only when something asks: `Bake` is the one stage that does, so
   `cargo art status` still answers on a machine with no Blender and
   `cargo art run --only concept` needs none either. `cli::plan` fingerprints
   only the stages it selected, which is what makes that true. A `run` whose
   plan does reach the bake reads the build before the loop starts, so a
   missing Blender is reported before the first paid stage rather than after
   three of them. `status` folds a build it cannot read into `State::Unknown`
   for that one row, with the reason, and an unknown row is not stale, so it
   drops out of the hint.

2. **A whole-struct digest of `Subject` was the wrong way to cover a future
   field.** The T13 row asks for `pose_mode`, which T12 has not added, so the
   requirement became "a field added later is covered". `format!("{:?}",
   subject)` does that in one line and also puts the description into the paid
   rig's fingerprint and the height into the paid concept's, which is the one
   thing the whole design forbids. So `fingerprint` destructures
   `CharacterSpec`, `Subject` and `Bake`, and each arm claims the fields it
   reads. Adding a field to any of the three is then a hard compile error,
   `E0027 pattern does not mention field`, not a lint anyone can miss; only
   the second step is a lint, because a field named in the pattern and claimed
   by no arm is an unused binding and `-D warnings` is what turns that into a
   decision. `Bake` is in it for the same reason: its five fields are split
   between the bake and the pack, and digesting it whole would re-render every
   frame to change a downscale target. `Remesh` and `Texture` stay
   whole-struct, because only the model stage reads either one, which is what
   the row asked for.

3. **The bake was deleting the mesh the rig record names.** `stages::bake`
   cleared `art/staging/<char>/` wholesale to drop stale frames, and
   `bare.glb` and `clean.glb` live in that directory. With those two in the
   rig's fingerprint, one bake made the paid rig stage read stale for good.
   The bake now removes the frames it is about to rewrite, `*.png`, and
   nothing else, which is all packing ever picks up. An existing test found
   it (`editing_a_sprite_setting_does_not_invalidate_the_paid_stages`) and
   `the_mesh_sent_to_rigging_survives_a_bake` is the test for the fix.

4. **The rig record is machine-local, on purpose.** `bare.glb` and
   `clean.glb` are what rigging is built from and neither is committed, so a
   fresh clone reads both as absent and `cargo art status` calls the rig
   stale. Nothing spends on that: while the committed `model.glb` is on disk
   `cli::plan` skips every stage up to `download`. Four commands can pay
   again, and each is typed by hand: `--from` or `--only` at `concept`,
   `model` or `rig`, plus `--retry`.

5. **The line between a required input and an absent one is who wrote it.**
   The row asks for an error on a missing input a stage needs. Applied to
   every input that would make `cargo art run` impossible on a new character:
   the four concept views are the model stage's input and the concept stage
   has not run yet. So the **committed** inputs are required, which is
   `humanoid.glb`, `humanoid.toml` and the scripts under `tools/blender/src/`,
   and everything the pipeline **produces** hashes as the literal word
   `absent` until it exists: the concept views, the two staging meshes, and
   every animation GLB including the three that happen to be committed.

6. **`LOCAL_PIPELINE_VERSION` is not bumped.** T13 changed no bake and no
   pack output, and the new content hashes already invalidate everything a
   bump would: `Model` gains the four views, `Bake` gains `model.glb`, the
   rig, the profile, the clips, the build and the scripts. `Pack` keeps its
   recorded fingerprint, and there is nothing about its output for a bump to
   express. The version stays at 3. `Stage::is_versioned` is where the set of
   three lives now, and it carries the warning the guard needs: a bump re-runs
   the model stage, and that spends Meshy credits.

   `Pack` is also the one stage whose fingerprint is not over everything it
   opens: it reads hundreds of staging PNGs and hashes none of them. That is
   deliberate, and it is safe for two reasons that hold together, not one.
   `Lock::record` clears every stage after the one it records, so a bake that
   runs always re-packs; and `stages::bake` deletes the `*.png` it is about to
   rewrite, so no frame of an older shape can be left for packing to pick up.
   Break either and hashing the frames becomes the fix.

7. **The committed `spec.lock` reports five stale stages, and is not
   rewritten.** `Concept` was already stale before T13: the recorded
   `57766a7c49b5516c` is not the digest of the current description plus pose
   instruction, under either scheme. `Model`, `Rig`, `Download` and `Bake` are
   stale because they now read files nothing hashed before, and `Pack` still
   matches. No fingerprint in that file is edited: T15 regenerates the
   survivor and rewrites all six, and a hand-written fingerprint would claim
   work nobody did. `Lock::states` answers for every stage in pipeline order
   and `cargo art status` prints what moved, because a reader told only
   "stale" deletes the lock and with it the task ids a rig was paid for.

8. **`Fetched` needed a fingerprint beside the verdict.** The criterion "an
   `[aim_table]` row invalidates every `Fetched` record" cannot be met by a
   verdict, which says what the gates found and not what fitted the clip. So
   the record also carries `lock::blender_inputs`: the canonical rig, its
   profile, the Blender build and every script, in one digest that `Bake`
   pushes as well. One function, so a row edited in `humanoid.toml` cannot
   invalidate the fit without also invalidating the bake. `fetch_plan` stays
   pure by taking a `cli::OnDisk` of the two maps it reads, and a step that
   wants a re-fetch names which of four reasons it is.

9. **A failing verdict is never one this pipeline wrote.** `stages::retarget`
   still refuses a clip whose gates found an error, so `cli::fetch` only ever
   records a passing one. The failing branch in `fetch_plan` is real and
   tested anyway, because `library.lock` is not gitignored, so it will be
   committed once written and will then travel between machines, and reading
   a failing record as cached would pass a clip nobody measured.
   `Verdict` records the report name, the worst severity and every rule that
   reported; the attempt number is the last segment of the report name, so it
   is not stored twice.

10. **`cargo art status` never recommends a command that spends.** The hint it
    prints is the only thing that tells a reader what to do about a stale
    record, and `cli::plan` returns `ConfirmSpend` only for a paid stage the
    lock still calls **current**: a forced paid stage that is already stale
    falls through to `Step::Run` and bills with no prompt. So the hint offers
    the earliest stale stage that does not `costs_credits()`, and lists the
    stale paid ones separately with what each would bill. Every paid stage
    runs before every free one, so `--from` a free stage cannot reach a bill.
    The better long-term fix is for `plan` to return `ConfirmSpend` for any
    forced paid stage regardless of `done`, since the prompt is about spending
    and not about caching. That is a change to T11's confirmation behavior and
    to the tests that pin it, so T13 left it alone and made the hint safe
    instead.

11. **The bake fingerprints `model.glb`.** It is the file the bake hands
    Blender as `--character`, and `cli::plan` judges every stage on its own,
    so without it a `model.glb` arriving from a pull left `Bake` reading
    `cached` with sprites of the previous mesh: fact 14's failure mode, moved
    off the rig and onto the character. It hashes as derived, `absent` until
    the download stage has produced it, so a character with no mesh yet can
    still be planned.

12. **Follow-up: a refused clip is left where the bake reads it.**
    `stages::retarget` writes the fitted GLB to its final path and only then
    refuses it for failing a gate, so a failed fetch leaves a clip the bake
    will happily play, with nothing in `library.lock` to say it failed. The
    next fetch reports it as `Changed` and refuses to replace it, and
    `--force` is the only way past. **Owner: the retarget failure path in
    `stages::retarget`**, which should write to a staging path and move it
    into place only after the gates pass, or delete what it refuses. Left for
    a later task: T13's fingerprints do not reach inside a stage, and no
    fingerprint change can fix a file written before the refusal.

### Corrections T14 made to this document

1. **`bake.pivot` cannot read the ground line, and reads the reflection
   instead.** The limits table asked for 1 px of ground-line drift across
   directions. Measured on the 848 rendered frames, the lowest content row
   moves **28 px** across the ring on `idle`, 86 on `run` and 52 on
   `walk_back`, and every one of those is correct geometry: a camera at 35
   degrees projects depth onto the vertical axis of the image, so turning the
   character changes the depth of its lowest foot and with it the row that
   foot lands on. Pairing opposite directions to cancel the depth still
   leaves 15 px, because the argmax moves to the other foot. A 1 px rule
   there would have failed every correct bake.

   What *is* exact is horizontal. An orthographic camera centered on the axis
   the ring turns about maps a world point at `x` to the mirror of where it
   maps it half a turn around, so for directions `d` and `d + count/2` the
   content spans satisfy `right(d) + left(d + count/2) = width - 1` and the
   same swapped, whatever the pose. Over the 424 opposite pairs of the three
   clips the worst reading of each is **1 px**, which the recorded run carries
   and a test pins. So the rule reads that
   identity, the published limit is 2 px with one pixel of rasterization
   headroom, and it still rejects the Test Plan's 20 px offset by 10x. The
   vertical is not left unmeasured: `clip.floor_snap` holds it in meters at
   the retarget, where the number means something, and `clip.root_bob` holds
   what the strip keeps.

2. **`bake.sampled_frames_are_keys` counts the frames *any* channel keys.**
   The strict reading, every channel, was implemented first and it failed all
   three committed clips at 14, 19 and 17 unkeyed frames. The reason is not a
   defect: of the 240 curves each clip carries, **91 key every frame** and
   **149** are a bone's own `location` and `scale`, which the glTF exporter
   stores as two endpoint keys. Filtering those out by value spread does not
   work either, and that was measured too: the 149 carry **2.8e-5 to 3.1e-5**
   of `f32` noise between their two keys while the smallest real motion in the
   same files is **3.59e-5**, a gap of 1.16x. No threshold separates them, so
   the honest question is whether anything authored a pose at the frame being
   rendered. The union answers that, it reads 0 on all three clips (15 frames
   of 47, 20 of 21, 18 of 23), and it rejects the Test Plan's negative.

   Those three zeroes are a **tautology on this art**, and the calibration
   column says so: each clip keys every integer frame of its own range, `idle`
   all 47 of 0..46, so no integer sample `framing.sampled_frames` can return
   is able to miss one. The evidence the rule reads anything is the synthetic
   negative, an action with every other key deleted, which reports the 3
   frames of 6 that nothing keyed.

3. **The projection had to be calibrated, and the first version was wrong.**
   `bake_sprites.bake_camera` read `camera.matrix_world` immediately after
   `setup_camera` placed it. Nothing had evaluated the scene, so the matrix
   was still the identity: the projection used the world axes, put the depth
   where the height belongs, and wrote a golden whose head sat 18 px **under**
   its own hips. `bpy.context.view_layer.update()` is the fix, and the test
   that would have caught it is now committed: all six goldens are read for a
   body the right way up, the head above the hips and the lowest joint a toe.
   Not the highest joint: `run_s` frame 19 swings a forearm past the head,
   which is why that half of the assertion is a set of toes and not a set of
   head bones.

4. **The goldens are 24 joints, not a chosen subset, and the columns are
   wider than the sketch.** The design's example shows `Hips` and `LeftHand`
   in a 12 character column. The rig carries `RightShoulder` at 13, and
   `render_size` can ask for a four digit canvas, so the format is
   `{frame:<7}{bone:<16}{x:>5}{y:>5}` under the same header. Every joint of
   the armature goes in, sorted by name, which is 24 rows per frame and 73
   lines per file: no subset has to be chosen and no second copy of the bone
   list exists. Six files, 438 lines, and the two directions are the first
   stop of the ring and the one three quarters around, which is `s` and `e` on
   every named ring the packer knows.

5. **The goldens reproduce byte for byte, twice, and they do not depend on
   the direction count.** Two `--update-goldens` runs of the committed clips
   wrote identical files, one at 16 directions and one at 4. That is stronger
   than a repeat: `direction_rotation` gives `e` the same angle in both rings,
   and the camera is framed over poses rather than directions, so the two runs
   share nothing but the math. A third run with the variable unset read them
   at **0 px on all 432 landmarks**. The committed goldens are therefore the
   goldens of the current art, which is what decision 11 asks for.

6. **The five Rust bake rules could not be calibrated on committed art, so
   the recorded run is.** The rendered set lives under `art/staging/`, which
   is gitignored, so no test can open it and CI cannot re-derive these
   numbers. `crates/xtask-art/tests/fixtures/bake.survivor.1.json` is
   therefore the whole bake report of a real run of the committed art: **31
   findings, every one `info`**, and the readings the `[profile.bake]`
   comments quote are in it. It replaces the 9 finding version T7 recorded.
   Every negative is a synthetic 64 px frame set in
   `crates/xtask-art/tests/unit/frames.rs`, which is what runs in CI.

7. **`bake.frame_count` counts what is absent from a rectangle, because the
   sampled frame count is Blender's own.** The design's calibration column
   says "directions x sampled frames", and Rust cannot know the second factor
   without a second copy of `framing.sampled_frames`. So the rule reads the
   rendered set as a rectangle whose width is the widest direction's own frame
   count, and reports how many of its cells are missing. One deleted file
   reads 1 and names it. The sampling itself is held to the action by
   `bake.sampled_frames_are_keys`, which is where that question belongs.

8. **`[profile.bake]` is four numbers, and three rules in the new families
   publish none.** `bake.frame_count` and the three `atlas.*` rules count
   defects, and `bake.forearm_roll` is a patch that must be off, so all five
   join the family whose limit is not a `[profile]` number, beside
   `rig.names_standard` and `concept.single_figure`. The limits table gained
   the six rows it was missing.

9. **`atlas.manifest_schema` calls the game's own reader, and the other two
   report its numbers.** `sprites::parse` is what Godot loads a manifest with,
   so the rule runs `sprites::parse` itself rather than filling the same types
   from RON: every invariant the game holds a manifest to is that one rule,
   and nothing describes the format a second time. `atlas.frame_count` and
   `atlas.trim_boxes` then report the readings behind two of those
   invariants, a rect count against the animation's own header and an anchor
   against its cell, plus the one thing no format check can see: whether a
   rect is inside the atlas image it indexes. A table test breaks each
   refusal `parse` has on a two-frame manifest and asserts some `atlas.*`
   rule calls it an error, so the split cannot drift back apart.

10. **The contact sheet is `preview::sheet`, written twice, and the
    `sprites.png` name is gone.** The full-resolution sheet is
    `art/preview/<char>/sheet.png`, gitignored, and the committed one is
    `project/assets/characters/<char>/sheet.png` under LFS, downscaled to
    2048 px on its longest edge. The survivor's own sheet is 3792 by 777, so
    the committed copy is 2048 by 420 and 365 KB, and every sprite in it stays
    128 px wide. One name for one picture at two resolutions, so the CI
    artifact and the file in the diff are obviously the same thing.

11. **CI can draw that sheet, so the artifact is real today.** The design says
    the full sheet is a CI artifact, and there is no CI job that bakes:
    fact 16 is that the runner has no Blender. But the sheet is read from the
    packed atlases, which are committed, so `cargo art check --sheet` draws it
    from them with no Blender at all. That is a flag on the one new verb
    rather than a second verb, the way `--list-rules` already is, and the
    `test` job runs it and uploads the result. `cargo art check` itself does
    not measure the atlases: the pack boundary owns those three rules, and a
    unit test reads the committed ones on every pull request.

    That redraw is also the staleness check. Two local runs write the same
    365 KB, sha256 `97e0cbb6`, so the job asserts the committed thumbnail
    came back byte for byte and a sign-off picture that no longer matches the
    atlases cannot ship. The comparison needs `lfs: true` on the checkout,
    which the `test` job already passes, or it would read a pointer.

12. **`MARROWFALL_UPDATE_GOLDENS` is read in exactly one place, and Python
    reads no environment at all.** `stages::updating_goldens` reads it and
    passes `--update-goldens` on argv, so the script takes a flag like every
    other parameter. CI refuses the variable in a step that runs **before**
    the tests, and a unit test reads the workflow file to prove the step is
    there and comes first. A second unit test asserts the variable is unset in
    the run it is part of, so a developer who exports it cannot get a green
    local suite either.

13. **Nothing remained to route for the clip audition.** T7 already wrote
    `art/staging/reports/fetch.<clip>.<attempt>.json` with all six `source.*`
    readings, `stages::check_source` already refuses the clip before the
    retarget spends anything on it, and `cli::fetch` already keeps the
    downloaded FBX. What T14 added is the proof: a stubbed unattended
    `cargo art fetch` now asserts that the report is on disk at that exact
    path with `source.posture` among its findings, and the test is red when
    the file is removed. One thing is left as it is on purpose: the `verdict`
    on `Fetched` names the retarget's report and not the audition's. Both
    boundaries gate the same uncommitted file and the fetch refuses the clip
    if the first one fails, so a recorded clip passed both; recording two
    verdicts would change the lock schema T13 fixed for a fact the report
    already carries.

14. **The bake writes its report before it renders, and the rules that need
    the scene run before it too.** `bake_sprites.main` used to write the
    report and then render for two minutes. The golden needs the camera and
    the pivot, so the scene setup is now its own function, the two Blender
    side rules are measured against it, the report is written, and the render
    is the last thing that happens, skipped outright when the report already
    carries an error. A golden moved 5 px stops the bake in **3 seconds**
    against **107** for the clean one and writes no frame; the four Rust
    rules that read those frames then report undefined on every clip, which
    is what an empty staging directory is. A render that dies still leaves
    the whole report on disk. `stages::bake` then extends that report with
    the five Rust rules and rewrites it, so the file a reader opens is the
    whole measurement rather than the Blender half of it.

15. **The rule list is 66.** T13's 56 plus seven `bake.*` and three
    `atlas.*`. Blender reports 20 of them, four of which are at the bake:
    `clip.root_travel`, `clip.root_bob`, `bake.sampled_frames_are_keys` and
    `bake.landmark_golden`.

16. **Round 2: three figures this document carried could not be re-derived.**
    The frames the `[profile.bake]` numbers are measured on are **rendered**,
    not committed: `art/staging/` is gitignored, which correction 6 already
    says. `bake.non_empty` reads **4.1164, 4.8367 and 4.8912** percent, one
    per clip, and the 7.5161 the limits table quoted is in no fixture, because
    the rule records only the emptiest frame of each clip. And the
    deleted-frame negative names **`w 01`**, index 2 of an eight direction
    ring, not `sw 01`.

### Corrections T12 made to this document

**`pose_mode` stays unset.** It is adopted only if all four acceptance items
hold, and `a-pose` fails two of them while `t-pose` fails three. Every number
below was measured in this task, on three meshes generated from the same four
committed concept views, and every one names the report it came from.

1. **Step 0: the field exists, and the API validates it by name.** Free, three
   requests, each with an `image_urls` no fetcher can resolve so none could
   become a task. Without `pose_mode` and with `pose_mode: "a-pose"` the
   refusal is identical and mentions only `ImageURLs[0]`. With
   `pose_mode: "banana"` the same refusal gains a second clause:

   ```
   PoseMode must be one of [a-pose t-pose]
   ```

   So Multi-Image to 3D does inherit the field, it is not ignored, and the two
   legal spellings are exactly the two the design guessed. `Client::probe` is
   what sends it: `submit` turns a 400 into an error and reads a task id out of
   a success, and here the 400 **is** the measurement.

2. **The four acceptance items, per mode.** `unset` is the request the
   survivor was built with. Reports:
   `art/staging/reports/{mesh,cleaned,cleanup,rig}.survivor-<mode>.1.json`.

   | Item | unset | `a-pose` | `t-pose` |
   |---|---|---|---|
   | 1. `rig.humerus_angle` within 15 of 40 | **19.1 / 19.5**, fail | **5.1 / 5.6**, pass | **31.7 / 30.6**, fail |
   | arms below horizontal | 59.1 / 59.5 | 45.1 / 45.6 | 8.3 / 9.4 |
   | 2. elbow bend under 5 deg | **24.0 / 23.5**, fail | **28.0 / 27.7**, fail | **18.5 / 19.1**, fail |
   | 3. welded `mesh.holes` on `bare.glb` le 200 | **31**, pass | **1288**, fail | **999**, fail |
   | 4. no invented forearm surface | a human reads `sheet.png` | same | same |

   **No mode passes item 2.** Meshy's auto-rigger puts the elbow joint off the
   arm line whatever pose the mesh arrives in, and the committed rig's own 24
   degrees (fact 1) turns out to be the middle of the range rather than a
   defect of that one generation. That is why `rig.elbow_bend` records instead
   of gating.

3. **Every `mesh.*` reading, on the real bare mesh at last.** `bare` is as
   downloaded, `clean` is what the fixer wrote:

   | Rule | unset bare / clean | `a-pose` bare / clean | `t-pose` bare / clean | limit |
   |---|---|---|---|---|
   | `mesh.holes` | **31 / 18** | 1288 / 230 | 999 / 352 | 200 |
   | `mesh.non_manifold` | **6** | 206 | 47 | 10 |
   | `mesh.islands` | **4 / 1** | 49 / 4 | 16 / 3 | 8 |
   | `mesh.self_intersect` | **1094 / 1153** | 4002 / 5382 | 4934 / 5653 | 1500 |
   | `mesh.mirror` | **3.257 / 0.000** | 2.040 / 0.000 | 2.340 / 0.000 | 3.5 |
   | `mesh.world_size` | **11.588 / 11.435** | 11.260 / 11.260 | 11.661 / 11.570 | 25 |
   | `mesh.budget` | **54,909 / 55,553** | 58,546 / 60,279 | 57,691 / 58,149 | 300,000 |
   | `mesh.non_manifold_post` | **17** | 74 | 71 | 20 |
   | `mesh.cleanup_effective` | **19 lt 35** | 234 lt 1337 | 355 lt 1015 | before |
   | `facing`, `stray_object`, `texture`, `uv`, `quads` | 0 | 0 | 0 | 0 |

   Setting `pose_mode` at all costs topology, heavily. `a-pose` returns **41x**
   the holes of the unset request and 8x the crossings; `t-pose` 32x the holes.
   Neither is a limit that wants widening: the fixer cannot close 1288
   boundary edges without leaving 74 non-manifold ones behind, and it does not.

4. **`mesh.world_size` was gated on a number nothing before rigging sets.**
   `height_meters` is a parameter of the **rigging** call, not of generation:
   Meshy returns a bare mesh in its own frame, centered on the origin, and all
   three read 1.8914 to 1.8982 m against the 1.700 the spec asks for, 11.26 to
   11.66 percent out. Read against `rig.world_height`'s 5 percent band, which
   is what the rule used, **every real bare mesh fails it**, and the committed
   `model.glb` reads 1.6999997 only because rigging scaled it. So the rule has
   a band of its own now, `[profile.mesh] height_percent = 25`, which leaves
   2.1x over the worst reading and still rejects the 100x node scale of fact 7
   at 9900 percent. `rig.world_height` keeps the 5 percent, and it holds: the
   three rigs read 1.835, 1.570 and 2.502.

5. **The bare mesh has no object name, and `[profile] meshes` was calibrated
   on the name rigging invents.** Meshy's `multi-image-to-3d` GLB carries one
   mesh node with **no `name` at all**. `char1` is created by the rigging
   endpoint, and the row named only that, so the fixer refused every real bare
   mesh with "the profile allows ['char1'], which is none of the 1 objects in
   this file". One object under three names, all measured: `node 0`, which is
   what `check/gltf_world.rs::node_name` calls an unnamed node; `Mesh_0`,
   which Blender names it on import and the fixer exports; and `char1` after
   rigging. All three are in the row.

6. **`rig.elbow_bend`, a fourteenth rig rule, records and never gates.**
   Acceptance item 2 needed a number no rule reported. It reads the angle
   between the shoulder-to-elbow direction and the elbow-to-wrist one, in
   world space, and files `info` against a ceiling of 180 degrees, the same
   shape `source.wander` and `source.posture` already use. A published limit
   would fail the committed rig at 24 degrees on every run, and nothing here
   can regenerate that rig. The rule list is **67**.

7. **The spike is the one command that measures past a failing mesh gate.**
   The rig stage refuses, and that is untouched and still tested: rigging a
   mesh that failed a gate wastes credits on art nobody will ship. Inside
   `cargo art spike-pose` the limits are what is being calibrated, so a mode
   is measured whichever way its gates went, the verdict is printed beside it,
   and the command exits non-zero carrying it. Free to re-run: a mode whose
   `bare.glb` or `rigged.glb` is on disk is measured again and bought again
   never, which is what makes 105 credits a one-time cost. It asks before it
   bills, the way every paid verb does, quoting the credits it has left to
   buy, and `--yes` answers that.

8. **`a-pose` fixes the worst rig defect this project has, and it is still not
   adopted.** Fact 1's sideways `Hips`, 97.8 degrees off the direction to its
   own child, is not an artifact of one bad generation: the unset regeneration
   reproduces it at **95.7**. With `pose_mode: "a-pose"` the same rig reads
   **5.8**, `rig.child_axis Hips` falls from 95.7 to 6.1, and the humerus
   lands inside its band for the first time. That is a real finding and it is
   recorded here rather than acted on, because the mesh it is built from
   carries 1288 boundary edges and 4002 crossing faces. If a later task finds
   a way to hold the topology, this is the row that says where to look.

9. **What the contact sheets show, reported and not decided.** Five views of
   each `bare.glb`, at `art/staging/survivor/spike/<mode>/sheet.png`: front,
   back, left, right, and the arms alone from 45 degrees above the front,
   framed on the tenth of the vertices furthest from X = 0, which in any
   arms-out pose are the hands and forearms. No mesh shows a flat invented
   plate where a forearm's top should be; all three carry rounded, textured
   forearms. `unset` hangs the arms nearly vertically and close to the body,
   `a-pose` holds them about 35 degrees down and out, and `t-pose` holds them
   horizontal with visibly thinner, flatter hands. Item 4 is the
   orchestrator's call on those images.

10. **The recorded Model and Rig task ids answer 404 under this key.** T10
    left a recipe in `crates/xtask-art/README.md` for fetching the survivor's
    own paid `bare.glb` for 0 credits. It cannot work: the tasks belong to
    another account, and every measurement here comes from the three
    generations this task paid for. That section is replaced by the spike's.

11. **Balance: 1161 before, 1056 after, 105 spent**, which is the estimate
    exactly: three generations at 30 and three rigs at 5. Step 0 and every
    balance read cost nothing. One generation was paid for twice over in
    wall-clock terms and not in credits: the first run died to a `SIGPIPE`
    after submitting, and the task was recovered from the API by hand.

    **That recovery is now the pipeline's, not a person's.** Every paid call
    goes through `Client::run`, which writes the task id to
    `art/staging/reports/<stage>.<item>.<attempt>.task` the moment `submit`
    returns and before the first poll, resumes that id instead of submitting
    when it is on disk, and removes it once the task reports success. So a
    death anywhere in the window that `SIGPIPE` landed in costs one poll
    rather than 30 credits. It is cleared on success rather than later,
    because `--retry` has to be able to buy a genuinely new task, and on a
    task the provider gave up on, because that one would read the same way
    forever. A poll that never landed clears nothing: resuming is what it is
    for.

## Documentation Changes

- `art/skeletons/README.md`: `[profile]`, `[aim_table]`, the new bone names,
  and the "regenerate in one deliberate operation" sequence, which is also the
  only way `humanoid.glb` is promoted.
- `art/characters/README.md`: `model.glb` is the rigged, skinned file the rig
  stage writes, and `art/staging/<char>/bare.glb` is the mesh before rigging.
- `crates/xtask-art/README.md`: the `model` stage now downloads and cleans,
  `check` is a new verb, the free and paid split changes, the concept retry
  loop is documented, the stale `art/pipeline/` reference goes, the three
  stage boundaries the `source.*` and `clip.*` rules run at are named, one
  table says what each stage's fingerprint reads, and one section names every
  command that can pay again and what needs Blender. **T12 replaces "Still
  waiting on a Meshy key" with "Which `pose_mode` to send"**, because the
  recipe in it cannot work and the recalibration it was for is done, and says
  where a submitted task id waits while that task is in flight.
- `README.md`: `cargo art check`, the two new spec fields, one line saying
  gates run at stage boundaries, and the E2E tier row changes from "nothing
  yet" to `render`.
- `tools/blender/README.md` (new, short): why `transfer.py` and `plant.py`
  never import `bpy`, why the fixer measures nothing, the rules each module
  reports, why a script never names a limit, and why the root strip works in
  world space. **T12 adds `mesh_sheet.py`**, which measures nothing either:
  it renders the five views of the model contact sheet and a human reads
  them.
- `docs/research/agent_reports/audit_the_current_art_pipeline.md`: a
  correction note. Its mesh numbers are the seam-split reading.

## Development Environment Changes

- Bun added to the `Brewfile` and to `setup`, with the npm `gltf-validator`
  pinned in `bun.lock`. There is no Homebrew formula for the validator and the
  published binaries are x64 (fact 16). Bun installs it and runs it, so the
  repository needs one JavaScript runtime rather than a runtime plus npm.
- `opencv-python-headless` added to the `uv` dependencies.
- `gltf` and `parry3d` crates added to `crates/xtask-art/Cargo.toml`.
  `gltf` gains its `utils` feature, which adds the accessor readers the mesh
  gates need and pulls in no new dependency. `parry3d` is pinned at 0.30.2
  with default features off.
- `[profile.mesh]` names the file each row was read on. Six of its nine rows
  are calibrations, measured by T12 on the real bare mesh and the file the
  fixer wrote from it. The other three are not calibrations: `triangles` is
  Meshy's stated rigging limit, `printability_edges` comes from the 179 this
  document records, and `height_percent` is a band wide enough to admit a
  mesh nothing has scaled yet.
- `pyproject.toml`: `--cov` over every `bpy`-free module with
  `--cov-fail-under=100`. T7 adds `source.py` to that list, making six:
  `clip`, `findings`, `framing`, `skeleton`, `source` and `transfer`. T14 adds
  no module: the landmark golden and the frame-key count are `framing.py`,
  which is the bake's own geometry and already in that list.
- `MARROWFALL_UPDATE_GOLDENS`, unset by default. Set to `1` a golden is
  rewritten, and CI asserts it is unset. **T14 built it:** `stages::bake` is
  the one reader, it passes `--update-goldens` on argv so no script reads an
  environment, the rule reports `skipped` on a run that rewrote its own
  golden, and both halves of the CI assertion are unit tested.
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
| T3  | Mesh measurement in Rust, calibrated on the mesh in hand | 2 d | **First: get the mesh, take it to world, then weld.** `bare.glb` was not downloadable, so the calibration asset is `model.glb` and every limit is provisional. Report a merge histogram and pick the plateau. Then `check/gltf_mesh.rs` for the surface and `check/mesh.rs` for thirteen rules: holes, non-manifold, islands, self-intersections via `parry3d`'s `Bvh`, mirror distance, world size, facing, stray objects, budget, UV bounds, texture, primitive modes, and `print/analyze` mapped into the report. Write every limit into `[profile.mesh]`. | The weld distance is chosen from the histogram, not assumed. Every limit in `[profile.mesh]` is a real number with its headroom published. Every `mesh.*` row except `non_manifold_post` rejects its negative fixture in CI with no Blender. | T1, T2 |
| T4  | Aim table and role map | 1.5 d | Add `[retarget_chain]`, `optional_roles`, `[fingerprints]` and `[aim_table]`. `skeleton.py` reads the whole file for the transfer and `check/aim.rs` reads the table for the gate, so each side refuses what it reads. Implement the three table validations, the `rig.aim_table` rule, and the skip-unmapped-ancestor walk over the chain. Checked against a synthetic fixture and against both rigs, and the real rig is renamed in T5. | No second copy of the map exists. A missing row, a row no convention maps, and a broken mirror pair each fail to load, on both sides. An out-of-band aim is `rig.aim_table`, which `--list-rules` prints and which reports 22 subjects per rig. | T2 |
| T5  | The transfer, the rename, and the refit | 3.5 d | `transfer.py` with no `bpy`: quaternion swing-twist aim application projecting the **vector part** per fact 19, `swing_singular` raised at 180 degrees, roleless bones skipped, separate `ref_world_*` dicts, algebraic local matrices, rotation-only keys for non-root bones, typed errors, LINEAR and CONSTANT, no reference-frame key. Delete `rebase_action`, `sole_children`, `align_to_world`, `bind_pose_mismatch`, `bone_directions` and the twelve self-referential tests. **Rename the committed `humanoid.glb` bones and refit `idle.glb` and `run.glb` in this PR**, because the rename invalidates them and this is the first task with the new retarget. | The three numeric known-answer tests pass: 10 deg twist to 10.00 and 0.00, 30 deg swing to 0.00 and 30.00, `Offset(LeftUpLeg)` about 174 and not identity. The 180 degree case raises `swing_singular`. A rig with `head_end` is skipped, not raised on. Requirements 1, 2, 7, 8, 9, 10 each have a passing test and a rejected negative. The 1.5x scale test gives identical output bone lengths. | T1, T4 |
| T6  | Clip verifier: swing and twist | 1.5 d | `check/clip.rs`: `clip.swing` absolute against the vendor file, `clip.twist` as the change from the roll the two bind poses call for, both from one vector-part split of `source^-1 @ output` about the bone's own +Y, over every mapped bone, frames aligned by seconds. `clip.object_transform` against the committed rig's own object nodes, and against any channel that drives one. `check/gltf_clip.rs` samples the delivered GLB and `check/motion.rs` reads the source sidecar `retarget_animation.py` writes. **Build a synthetic cross-rig fixture and measure both limits on it**, cross-checked by hand against the three Mixamo clips. | Both limits are written into `[profile]` from a measurement, not assumed. The Rust split passes the same two numeric cases as the Python one. `clip.swing` reproduces the shipped `strafe_left.glb` at 97.797 and stays quiet on a correct fit. `clip.twist` rejects the post-multiplied 90 deg twist, and `clip.swing` stays under its limit on that same fixture. A 180 degree swing reports an error, never a NaN. | T5 |
| T7  | `source_fps`, `travels`, traveling fetch, root travel | 1.5 d | Add `Animation::source_fps` and `Animation::travels`, filling `source_fps` from each vendor file and **declaring `travels` for every clip**: `false` for `idle`, measured for `run` because a Meshy library clip is likely in place, `true` for the three Mixamo clips. Scene fps equals `source_fps`, with the key grid and range asserted as Findings. Request traveling export from Mixamo and add `source.traveling`, **symmetric on 0.02 m of hip travel in both directions**. Add `source.child_axis`, `info` only, measuring each source bone's own axis against the direction to its mapped child, because a vendor skeleton is not ours to fix and correction 6 is the reason it has to be on record. Move `clip.root_travel` to the bake boundary as a per-axis maximum on the stripped copy, splitting the up axis off as `clip.root_bob` because the strip keeps it (correction 2), and record the excursion the strip removes as `source.wander` at the fetch (correction 1). Delete the `array_index == 2` branch, `loop_mismatch` and `report_loop`, and add `clip.loop`. | Requirement 3 holds and the 0.8 to 16.8 fixture is rejected. `source.traveling` rejects an in-place export declared `travels: true` **and** a traveling export declared `travels: false`, so a mistyped flag cannot skip the gate. `run`'s `travels` is a recorded measurement, not a default. Root travel after strip is under 2 cm on both horizontal axes and the bob under 15 cm on the up one. `source.child_axis` records Mixamo's `Neck` at 16.933 degrees and its `Hips` at 7.051. No sprite rate changes. | T5 |
| T8  | Floor snap, and the femur band | 1.5 d | **T5 already moved the metric**: `stride_segment` in the skeleton file names the two roles, and every location key is scaled by that ratio in one operation. What is left here is the floor: snap the lowest foot frame to Z equals 0 and report `clip.floor_snap`, and hold travel to a band rather than to a printed line. | Requirement 4 holds. Travel matches the source within 2 percent, which T5 measures by hand at 2.3117 m times 0.8815 giving 2.0378 m and does not yet gate. `clip.floor_snap` is under 5 mm, and the fixture with the snap removed is rejected. | T7 |
| T9  | Foot planting | 3 d | `plant.py`: contact detection at the published thresholds, scaled to character height and expressed as a rate, a majority vote whose width is odd and at least 3, foot XY lock, two bone analytic IK, ramps. | All three `foot_contact` sub-rules have a row and a rejected negative. Plants is an error at zero runs. Skate under 2.5 cm and penetration under 5 mm on every clip. The same toe path at 8 and 30 fps gives the same runs, and the vote width is odd and at least 3 at both rates. | T8 |
| T10 | Cleanup, symmetrize, and the post-cleanup ceiling | 2 d | `Subject::cleanup` and `Subject::symmetry`, false by default, true for the survivor. `mesh_clean.py` in world space, measuring nothing. The `model` stage downloads `bare.glb`. Rigging sends `model_url` as a data URI with `input_task_id` omitted. **`bare.glb` could not be downloaded here either**, so the fixer is built and measured against a stand-in lifted out of `model.glb`, every `[profile.mesh]` row stays provisional, and the 5-credit call is left with its steps written down. | The fixer runs, and `cleanup_effective` shows the classes it counts strictly decreasing: holes 171 to 73 and pieces 7 to 2 on the stand-in. `non_manifold_post`'s ceiling is a measured 12 with 8 spare. The file rules run on both meshes, so texture and UVs are measured on the file that gets rigged, and **that file fails `mesh.self_intersect` at 1026 against 1000**: the honest signal correction 1 is about. The no-op stub is rejected. With `symmetry: false` the fixer skips the mirror, the three mirror rules report `skipped`, and their negative controls still run. **Blocked on a key: the recalibration on the real `bare.glb` and the real 5-credit rigging call by data URI**, both of which T12 did. | T3 |
| T11 | Concept gates, calibration and the retry loop | 1.5 d | **First: measure the five `concept.*` rules on the four committed views and write the limits with their headroom into `[profile]`.** Then `concept_check.py`: background, one figure, arm gaps, mirrored silhouette when `symmetry` is on, and `cross_view` across the four views. Then the retry loop in `cli.rs`, three attempts total, a fresh set of views on each, numbered reports. Delete `pause_for_review` and `should_pause`. | No `concept.*` threshold is guessed. Each rule rejects its negative fixture. Three failures leave three numbered reports and bail with the images on disk. A pass on attempt two proceeds, and the test asserts attempt two generated again rather than reusing what failed (correction 13). `ConfirmSpend` quotes 2.40 USD once. No Meshy stage is wrapped. | T1, T10 |
| T12 | `pose_mode` spike | 0.5 d, 105 credits spent of 90 estimated | **Done.** Step 0 read the API's own refusal: the field is inherited, validated by name, and takes `a-pose` or `t-pose`. `Subject::pose_mode` rides in the `Model` fingerprint, `cargo art spike-pose` runs all three end to end, and T10's blocked recalibration is done on a real bare mesh at last. `mesh.world_size` gained a band of its own and `[profile] meshes` three names, both for measured reasons. | **The field stays unset.** `a-pose` fails two acceptance items and `t-pose` three; no mode passes the elbow one. Every number, both balances and what the three contact sheets show are in the T12 corrections. `rig.elbow_bend` is the fourteenth rig rule, recording. | T2, T3 |
| T13 | Lock fingerprints real inputs | 1 d | Hash `humanoid.glb`, `humanoid.toml`, the concept PNGs, `model.glb`, every animation GLB, the two staging meshes, the Blender build and every script into the right stages, and destructure `CharacterSpec`, `Subject` and `Bake` so a field added later cannot be forgotten. Add `Model` to the version guard. Add `verdict` and `fingerprint` to `Fetched`. | `humanoid.glb` invalidates retarget and bake but not `Rig` or `Model`, so a local rename spends nothing. A concept PNG invalidates `Model`. A replaced `model.glb` invalidates `Bake` and nothing paid. An `[aim_table]` row invalidates every `Fetched` record and the character lock's `Bake`. A Mixamo clip's verdict is readable in `library.lock`. `cargo art status` reports without Blender and never recommends a command that spends. | T1 |
| T14 | Bake and atlas gates, sheet, goldens | 2 d | Seven `bake.*` rules including `sampled_frames_are_keys`, and three `atlas.*` rules. Commit the downscaled contact sheet under `project/assets/characters/<char>/` and upload the full one as a CI artifact. Landmark goldens, 3 frames by 2 directions per clip. Route the clip audition into the fetch report. | Every `bake.*` and `atlas.*` row rejects its negative fixture. A wrong arm shows as a changed number in the diff. A missing golden fails. The audition numbers survive an unattended run in `reports/fetch.<clip>.1.json`. CI asserts `MARROWFALL_UPDATE_GOLDENS` is unset. | T1, T6, T7 |
| T15 | Regenerate the survivor, flip gates to required | 1.5 d, ~35 credits | One deliberate operation on the `bare.glb` **that T12's winner produced**: clean, symmetrize, re-rig, promote to `art/skeletons/humanoid.glb` per its README, refit `idle.glb` and `run.glb`, refetch the three Mixamo clips traveling, re-bake, re-pack, re-golden, re-sheet. Delete `apply_forearm_roll`. Then make the `required` aggregator the single required check, pinned by `app_id`. **Regenerate a second time if the first pass teaches something.** | Every gate passes on the regenerated art with zero waivers. Cost recorded: paid is rigging 5 credits plus image-to-3d 20 to 30 only if T12 adopted a `pose_mode`, about 0.45 USD at 0.013 per credit. Free is `print/analyze`, the cleanup, the Mixamo refetch, the retarget, the bake, the pack and the goldens. `model.glb`, `humanoid.glb`, `idle.glb`, `run.glb`, every atlas under `project/assets/characters/` and the sheet move in one PR. | T5, T6, T7, T8, T9, T10, T11, T12, T13, T14 |
| T16 | Godot e2e smoke test | 2 d | Fill the empty e2e tier: launch Godot headless, load every atlas and manifest, grep the log for `SCRIPT ERROR`, a load failure and a leaked object. Add the `pkill` watchdog, because Godot hangs rather than exits on a fatal error. | A deliberately corrupted manifest fails the test. Headless loads only, never pixels. The `README.md` tier table names `render`. | T14, T15 |
