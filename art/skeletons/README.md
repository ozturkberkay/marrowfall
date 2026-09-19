# Skeletons

A skeleton is the shared rig. Characters are built onto it, animations drive
it, so both are interchangeable.

Two files:

- `humanoid.glb` is the rig itself: 24 bones, no fingers, no mesh.
- `humanoid.toml` is the rulebook. Which bone fills which body part in each
  vendor's naming, where every bone must point, and the limits a rig has to
  pass (`[profile]`).

`cargo art check` measures a character against `humanoid.toml` and prints one
line per problem. Zero means it passes.

## Replacing the rig

```bash
cargo art run survivor --from model    # generate, clean, gate, rig, conform
cargo art promote survivor             # model.glb -> humanoid.glb
cargo art fetch                        # refit every clip onto the new rig
cargo art run survivor --from bake     # re-bake, re-pack, re-sheet
```

`promote` refuses a rig that fails a check, because every character would
inherit the fault. The refit matters: an animation stores rotations relative
to a rest pose, so a new rest pose means every clip has to be fitted again.

`../../crates/xtask-art/tests/fixtures/humanoid_before_rename.glb` is an old
copy of this rig, kept as a deliberately-wrong input the checks are tested on.
