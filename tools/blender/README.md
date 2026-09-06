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
| `transfer.py` | no | the retarget maths: world matrices in, local poses out |
| `clip.py` | no | what the retarget reports: the two counts, the frame grid, and the source motion sidecar |
| `source.py` | no | what a vendor clip is, measured before anything is fitted to it |
| `skeleton.py` | no | the skeleton file: roles, the retarget chain, the aim table |
| `framing.py` | no | the bake's camera geometry, frame sampling, and the root strip |
| `findings.py` | no | the Finding record, the rule, the report, and the success sentinel |
| `check_source.py` | yes | imports the downloaded FBX and hands it to `source.py` |
| `retarget_animation.py` | yes | imports two rigs, drives `transfer.py`, writes keys |
| `bake_sprites.py` | yes | renders the sprite sheet |
| `strip_animation.py` | yes | drops the mesh a provider ships with a clip |

The six `bpy`-free modules are unit tested by `uv run pytest` at 100 percent
coverage, with no Blender anywhere. That split is not tidiness: `bpy` only
exists inside Blender, so a module that imports it cannot be tested at all.

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

## Why one script writes a sidecar

`clip.swing` and `clip.twist` measure the delivered GLB against the file the
motion was bought in. That file is an FBX, the Rust `gltf` reader cannot open
one, and there is no Blender in CI. So `retarget_animation.py` writes the
source clip's own world orientations to
`art/staging/reports/retarget.<clip>.1.source.json`, and
`crates/xtask-art/src/check/clip.rs` reads that beside the GLB. The record
itself is built in `clip.py`, with no `bpy`, so it is unit tested like
everything else here.

## Why every export asks for the rest position

`clip.twist` reads its rest term off the joints of the file it is handed, so
the armature has to leave Blender at its rest position rather than at whatever
frame the scene is on. The exporter does that by default, and a default can
move, so `retarget_animation.py` and `strip_animation.py` both state
`export_rest_position_armature=True`. With it off, a correct `strafe_left` fit
reads 113.884 degrees where it should read 11.411 and 20 of its 22 roles go
red. A unit test reads every script here and fails on an export that leaves
the flag out.

## Running one by hand

The Rust side owns the invocation, in `crates/xtask-art/src/blender.rs`, and
one tested function builds every argument list. `art/staging/reports/` keeps
the argv verbatim beside each report, so a failed run can be repeated exactly.
