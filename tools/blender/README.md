# Blender scripts

Python exists in this repository for one reason: Blender is scripted in Python
and nothing else. Everything the pipeline can measure without Blender is Rust,
under `crates/xtask-art/src/check/`.

## Four rules

1. **A gate needs a negative control.** No check merges without a fixture it
   is proved to reject. Where real broken art exists, that is the fixture.
2. **A gate needs a calibration.** Known-good art in, silence out, and every
   rule names the representation and the space it measured in.
3. **A fixer is never trusted by its return code.** Every repair is followed
   by re-running the gate that asked for it.
4. **CI-side measurement is Rust.** Python measures only what needs `bpy`,
   because there is no Blender build for this project's CI runner and rule one
   is worthless if a negative control cannot run there.

## Which files import `bpy`

| Module | `bpy` | What it is |
| --- | --- | --- |
| `cleanup.py` | no | what the mesh fixer does, in which order, and which pieces are debris |
| `transfer.py` | no | the retarget maths: world matrices in, local poses out |
| `plant.py` | no | where a foot touches the ground: contact, the lock, the leg solve |
| `clip.py` | no | what the retarget reports: the two counts, the frame grid, where the fit ended up, and the source motion sidecar |
| `source.py` | no | what a vendor clip is, measured before anything is fitted to it |
| `skeleton.py` | no | the skeleton file: roles, the retarget chain, the aim table |
| `framing.py` | no | the bake's camera geometry, frame sampling, and the root strip |
| `findings.py` | no | the Finding record, the rule, the report, and the success sentinel |
| `actions.py` | yes | the F-curve edits the retarget and the bake both make |
| `check_source.py` | yes | imports the downloaded FBX and hands it to `source.py` |
| `mesh_clean.py` | yes | welds, drops debris, fills holes and mirrors the bare mesh |
| `retarget_animation.py` | yes | imports two rigs, drives `transfer.py`, writes keys |
| `bake_sprites.py` | yes | renders the sprite sheet |
| `strip_animation.py` | yes | drops the mesh a provider ships with a clip |

The eight `bpy`-free modules are unit tested by `uv run pytest` at 100
percent coverage, with no Blender anywhere. That split is not tidiness: `bpy`
only exists inside Blender, so a module that imports it cannot be tested at
all.

`transfer.py` and `plant.py` are the two that most need it. Both are pure
geometry with an answer that is right or wrong by a number, and both are
wrong in ways that look plausible: a retarget composed in the wrong order
still produces a posed character, and a foot solve that picks the wrong bend
plane still produces a leg. Neither needs a scene, only matrices and points,
so `retarget_animation.py` reads Blender and hands them plain numbers, and
every case they answer is a pytest one that runs in CI where there is no
Blender at all.

## Three things a script never does

**It never decides its own exit code from its own findings.** It measures,
writes its report through `findings.write_report`, and finishes. The Rust
runner reads that report and decides. Blender exits 0 when a script raises
from a handler, a thread or `atexit`, so the exit code cannot say whether a
run finished: `findings.guard` writes a success sentinel as its last act and
Rust asserts the file exists.

**It never names a limit.** Every published limit is `[profile]` data that
Rust reads and validates once, so the runner passes `--limit RULE=NUMBER` for
each rule a script reports and `findings.Rule` reads it back. A script that
named its own would be a second copy of a number `--list-rules` prints, and
`Report::off_registry` refuses a report that disagrees with that list anyway,
down to the comparison and the severity.

**It never applies an object transform to a rig that owns an action.**
`transform_apply` rescales the rest geometry and leaves every location key
byte identical, so their meaning changes by the object's scale and 2.316 m of
travel reads as 231.599 m. World matrices are composed instead, as
`matrix_world @ pose.matrix`, and written back through
`matrix_world.inverted()`. A unit test reads every script here and fails on
the call, because the last caller was deleted and nothing else would notice
it coming back.

## Why the root strip works in world space

The root bone's `location` channels are in its own rest axes, and on this rig
`Hips` local Z is world minus Z tilted 8.9 degrees. Pinning channels 0 and 1
and keeping channel 2 therefore sinks a left strafe by 0.32 m and lifts a
right strafe by 0.39 m, against a real bob of 0.043 m. `strip_root_motion`
takes every key to world space, pins it there with `framing.pin_horizontally`,
and brings it back: measured, the three committed clips then read about 2
nanometers on both horizontal axes, against 0.0428 m on X under the strip this
replaces.

The up axis is kept, because a run's bob is animation rather than travel, and
flattening it would leave a jump permanently on the ground. So it has a rule
and a limit of its own, `clip.root_bob` at 0.15 m: the fitted clips read
0.0089 to 0.0535 there, and the 0.2911 m the old strip sank a left strafe by
is still refused.

## Why the floor is not zero

`clip.floor_snap` puts a clip's lowest toe frame where the rig's own rest pose
puts it, which on this skeleton is **0.0307 m up**: the toe joint is the ball
of the foot and there is no toe-tip joint, so zero is where the sole is.
Measured, `model.glb`'s mesh spans exactly 0 to 1.700000 m in world space and
`LeftToeBase` rests at 0.031081. Snapping the joint itself to zero would sink
every clip three centimeters into the tile, and it would reject the committed
`run.glb`, which already stands 1.7 mm under that floor.

The lift is one constant per clip, taken over the whole clip rather than per
frame: a walk that never lifts its right foot is still standing on the ground
it plants its left one on. It is applied to the root's location keys in world
space, for the same reason `strip_root_motion` pins in world space, and the
pose is then evaluated again so the report is what the file carries rather
than what the lift was asked for.

## Why the foot is planted on its sole and not its toe

What `plant.py` is handed is a **sole point**, two per foot, under the ankle
and under the toe, and never the joint itself. Corrections 1 and 2 of T9 in
`docs/design/2026_08_20_art_pipeline_foundations.md` own the measurements that
say why, and correction 4 owns the model's limit.

The lock holds the ball still through a run, with two frame ramps either side,
and a two bone analytic solve moves the knee so the ankle carries the foot
there with its own world orientation held. A target outside the leg's reach is
reported as the distance it fell short by, never as a NaN.

## Why the bake scales nothing

`retarget_animation.py` sizes every location key once, by the femur, and
`clip.stride` measures that the fit travels what its source did. So a clip
reaching `bake_sprites.py` is already on this body: measured, the three
committed clips read **1.227e-6** away from the character's own rest height,
which is the `f32` a GLB stores a joint position in. The bake therefore
refuses a clip further out than `framing.SAME_BODY` instead of rescaling it,
because scaling there would size the same lengths twice.

## Why one script writes a sidecar

`clip.swing` and `clip.twist` measure the delivered GLB against the file the
motion was bought in. That file is an FBX, the Rust `gltf` reader cannot open
one, and there is no Blender in CI. So `retarget_animation.py` writes the
source clip's own world orientations to
`art/staging/reports/retarget.<clip>.1.source.json`, and
`crates/xtask-art/src/check/clip.rs` reads that beside the GLB. The record
itself is built in `clip.py`, with no `bpy`, so it is unit tested like
everything else here.

## Why the fixer measures nothing

`mesh_clean.py` welds `bare.glb` at the distance the gates weld at, drops
every object `[profile] meshes` does not name, drops the pieces under
`[profile.cleanup] smallest_island_cubic_meters`, fills what holes are left,
and mirrors the result when `spec.subject.symmetry` says so. It reports no
finding at all: rule three above is that a fixer is never trusted by its own
return code, so `crates/xtask-art/src/check/mesh.rs` reads the mesh it was
given beside the mesh it wrote.

**World space comes first**, before anything measures a size. Meshy's own
repair extension returned FINISHED having deleted nothing, because it measured
piece volume in local space while its checker measured world space, and this
asset family carries a 100x node scale. Then the weld, because glTF splits a
vertex at every UV seam and an unwelded survivor reads 13,368 boundary edges
where it has 171. Then the debris, before the holes are filled, because a hole
in a piece about to be deleted is not worth closing. `cleanup.ordered_steps`
is that order, as data, and the flag that adds the mirror to the end.

Measured on the stand-in described in correction 1 of T10 in the design, the
fixer takes holes 171 to 73, pieces 7 to 2 and the worst mirror distance 3.021
percent of width to 0.000, and it keeps the texture and the one UV layer.

The mirror costs what it costs. It copies the crossing faces of the half it
keeps, 905 to 1026, and leaves 33 more boundary edges and a second piece.
Correction 13 measures the three ways of taking that back, and takes none:
mirroring before the fill is worse, a weld of the seam alone recovers 6
percent of it, and no mirror threshold between 0.5 mm and 5 mm holds every
gate.

## Why every export asks for the rest position

`clip.twist` reads its rest term off the joints of the file it is handed, so
the armature has to leave Blender at its rest position rather than at whatever
frame the scene is on. The exporter does that by default, and a default can
move, so `retarget_animation.py` and `strip_animation.py` both state
`export_rest_position_armature=True`. With it off, a correct `strafe_left` fit
reads 113.884 degrees where it should read 11.411 and 20 of its 22 roles go
red. A unit test reads every script here and fails on an export that carries
an armature and leaves the flag out. `mesh_clean.py` exports no armature at
all, `export_skins=False`, so it is the one export with no pose to get wrong.

## Running one by hand

The Rust side owns the invocation, in `crates/xtask-art/src/blender.rs`, and
one tested function builds every argument list. `art/staging/reports/` keeps
the argv verbatim beside each report, so a failed run can be repeated exactly.
