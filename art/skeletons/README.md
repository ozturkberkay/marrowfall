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

The committed rig still carries the auto-rigger's own names and breaks seven
of these rules. It is regenerated in one deliberate operation, below.

## What it is for

An action stores each bone's rotation relative to its rest pose, so a clip only
plays correctly on a rig in the same rest pose with the same bone names. Motion
bought elsewhere is fitted to this file once, when it is fetched, by
`tools/blender/src/retarget_animation.py`. After that every file in
`../animations/` drives the same skeleton.

The fit pairs bones by role and never by name, out of `humanoid.toml`: this
rig's `Spine` is the highest of its three and Mixamo's is the lowest, so a name
match would drive the wrong bone.

## Regenerating it

`humanoid.glb` is the survivor's armature plus the one-triangle skin carrier
glTF needs to keep an armature at all. Every clip in `../animations/` is
authored against it, so regenerating it means refitting every clip in the same
change.
