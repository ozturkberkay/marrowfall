# Skeletons

Two files per skeleton. `humanoid.glb` is the armature every clip for it is
authored against, with no mesh: this project's standard biped, 24 bones, no
fingers. `humanoid.toml` beside it says which bone fills each anatomical role,
here and in every naming convention a clip can arrive in.

A skeleton is the third kind of shared art, beside `../animations/` and
`../characters/`. It belongs to neither: a character is rigged onto a skeleton,
and a clip drives one, but the skeleton outlives both.

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
