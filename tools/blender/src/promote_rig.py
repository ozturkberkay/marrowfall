"""Promote a conformed character to its skeleton's canonical rig.

`art/skeletons/<skeleton>.glb` is the armature every clip for that skeleton is
fitted onto, and it is the armature of a character that passed the rig gates:
same joints, same rest pose, no mesh and no motion. This is the one operation
that writes it, so regenerating a character and regenerating the rig every
clip is authored against are the same deliberate act.

The bind-pose action the vendor ships goes with the mesh. It keys the rest
pose the file already carries, so a canonical rig that kept it would hand
every reader an action that does nothing.

    blender --background --python tools/blender/src/promote_rig.py -- \
        --character art/characters/survivor/model.glb \
        --out art/skeletons/humanoid.glb
"""

import argparse
import pathlib
import sys

import bpy
from armature import export, keep_only
from findings import guard


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--character", type=pathlib.Path, required=True)
    parser.add_argument("--out", type=pathlib.Path, required=True)
    return parser.parse_args(argv)


def promote(character: pathlib.Path, out: pathlib.Path) -> None:
    """Writes `out` as the armature of `character`, at rest and alone."""
    if not character.exists():
        sys.exit(f"error: {character} not found")

    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=str(character))

    armature = next((o for o in bpy.data.objects if o.type == "ARMATURE"), None)
    if armature is None:
        sys.exit(f"error: {character} has no armature, so it rigs nothing")

    keep_only(armature)
    armature.animation_data_clear()
    for action in list(bpy.data.actions):
        bpy.data.actions.remove(action)
    export(out, armature)


def main() -> None:
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    args = parse_args(argv)
    promote(args.character, args.out)
    print(f"promoted {args.character.name} to {args.out}")


def save_blend(path: pathlib.Path) -> None:
    """The scene as it stood when the script failed, for a human to open."""
    bpy.ops.wm.save_as_mainfile(filepath=str(path), copy=True)


if __name__ == "__main__":
    # Blender exits 0 on an uncaught exception, so a failure here would look
    # like success and leave the canonical rig half written. `guard` writes
    # the success sentinel the Rust side asserts.
    guard(main, save_blend)
