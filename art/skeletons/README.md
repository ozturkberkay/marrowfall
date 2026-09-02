# Skeletons

Two files per skeleton. `humanoid.glb` is the armature every clip for it is
authored against, with no mesh: this project's standard biped, 24 bones, no
fingers. `humanoid.toml` beside it says which bone fills each anatomical role,
here and in every naming convention a clip can arrive in, and what a
conformant rig looks like.

A skeleton is the third kind of shared art, beside `../animations/` and
`../characters/`. It belongs to neither: a character is rigged onto a skeleton,
and a clip drives one, but the skeleton outlives both.

## `[profile]`, the rules a rig must obey

`humanoid.toml` carries one more table, `[profile]`, and it holds **every
published rig limit**: the exact bone set, the single root, the real
hierarchy, which child each bone's own axis must point at, the mirror
tolerances, the height band, and the angle the upper arm hangs at. It is data,
so a second skeleton is a second file rather than more code.

`crates/xtask-art/src/check/rig.rs` is the only reader. `cargo art check
--list-rules` prints the whole table as rules, and `cargo art check` measures
a character's rigged GLB against it and prints one line per defect. Bone names
are the Mixamo and HumanIK standard, with the spine numbered from the bottom,
so `Spine` is the lowest of the three.

Two rows describe the same hierarchy for two different reasons, the way
Godot's `SkeletonProfile` carries both. `[profile.parents]` is where each bone
hangs. `[profile.tails]` is the one child a bone's own axis must point at,
which a branch bone needs: `Hips` has three children and only `Spine`
continues the body.

The committed rig was renamed to these names, so `rig.names_standard`,
`rig.bone_set` and `rig.parents` now pass on it. Five rules still break, and
all five are geometry a rename cannot move: `rig.child_axis` on `Hips`,
`Spine2` and `Head`, `rig.mirror_length` and `rig.mirror_direction` on the
limb segments, `rig.humerus_angle` on both arms, and `rig.aim_table` on
`Hips`, which lives in `check/aim.rs` rather than `check/rig.rs` and shares
the prefix. `cargo art check` prints 14 defects across the five.
Regenerating the rig is what closes them, in one deliberate operation, below.

## `[aim_table]`, where every bone must point

The retarget aims both rigs at the same absolute directions, records the pose
each one reaches, and takes the rotation between them as that bone's constant
offset. `[aim_table]` is those directions: one row per role, in Blender Z-up
world space, with the character facing -Y, so +X is his left. Each row is a
direction rather than a unit vector, so the two readers normalize it and the
rows stay whole numbers.

**Every role has a row, torso included.** The code this replaces corrected
only bones with exactly one mapped child, so it silently skipped `Hips`, the
spine, `Head`, both hands and both toes, and shipped their rest twist into
every clip.

Two readers, one file. `tools/blender/src/skeleton.py` reads it for the
transfer and refuses a table with a missing row, a row no convention maps, or
a mirror pair that is not an exact reflection. `rig.aim_table`, in
`crates/xtask-art/src/check/aim.rs`, is the check that needs a rig: it
measures each aim against the rest pose the rig itself carries and reports
anything further out than `max_bind_deviation_degrees`. Measured against the
committed table, our A-posed rig is worst at 34.86 degrees and a T-posed
Mixamo rig at 45.01, while the committed `Hips` reads 97.80 because its own
axis points out of a hip socket. Our own figures are asserted against the
committed GLB. The Mixamo one was measured by hand on a downloaded FBX under
`../staging/`, which is gitignored, so no test can re-derive it.

## Three more tables the retarget reads

- `optional_roles` names the roles a source convention may leave out. The
  canonical convention has to fill them all, because it is what defines the
  role set.
- `[retarget_chain]` is the hierarchy the transfer walks, by role. It is not
  the rig's own bone hierarchy in `[profile.parents]`: a source with three
  spine bones drives a target with four, so the walk steps over any role the
  source leaves out. `hips` is the top and has no row.
- `[fingerprints]` names one bone per convention that no other convention
  has, so two identical role tables can still be told apart.

## What it is for

An action stores each bone's rotation relative to its rest pose, so a clip only
plays correctly on a rig in the same rest pose with the same bone names. Motion
bought elsewhere is fitted to this file once, when it is fetched, by
`tools/blender/src/retarget_animation.py`. After that every file in
`../animations/` drives the same skeleton.

The fit pairs bones by role and never by name, out of `humanoid.toml`. Both
convention tables name the same bones today, and they stay two tables: this
rig's `Spine` was the highest of its three until the rename, and the next
bought rig need not agree with either.

## Regenerating it

`humanoid.glb` is the survivor's armature plus the one-triangle skin carrier
glTF needs to keep an armature at all. Every clip in `../animations/` is
authored against it, so regenerating it means refitting every clip in the same
change.

## Renaming its bones

A rename is a JSON edit and a refit, in that order, and both belong to one
change. glTF addresses a joint by node index, so a bone's name is one string
in one place: rewriting the JSON chunk and leaving the BIN chunk alone keeps
every vertex, accessor and inverse bind matrix byte identical, which is what
lets the mesh calibrations in `[profile.mesh]` survive it.

1. Rewrite `nodes[].name` in `humanoid.glb`, in every character's
   `model.glb`, and in every clip under `../animations/`. All of them, or the
   bake stops: it refuses a clip that drives a bone the character does not
   have.
2. Update the canonical convention table in `humanoid.toml` to the new names.
3. Refit every clip under `../animations/` through
   `tools/blender/src/retarget_animation.py`, with the clip as `--source` and
   `--convention meshy`. Same body, so every offset is identity to about
   0.03 degrees and the posture does not move. That number is the check: a
   larger one means the rig and the clips disagree about the rest pose.

`../../crates/xtask-art/tests/fixtures/humanoid_before_rename.glb` is this
file as it stood before the last rename. It is the only negative control
`rig.names_standard`, `rig.bone_set` and `rig.parents` have, so it is
committed and not regenerated.
