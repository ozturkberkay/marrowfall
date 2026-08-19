"""Strip an animation GLB down to its motion.

Providers return the whole character with each animation: mesh, skeleton and a
2048-square texture, ~5 MB, of which ~40 KB is the motion. Engines store
animations without a mesh (Unity `.anim`, Unreal AnimSequence, Godot
`Animation`), and so do we, these files are committed, so the difference is
permanent in git history.

The armature stays. It is what the action's F-curves address, and the bake
compares it against the character's rig to catch a bind-pose mismatch.

glTF has no standalone armature: bones only survive as part of a skin, so an
armature exported alone is silently dropped. A single tiny triangle, weighted to
the root bone, is what carries it through. The bake discards it on import.

    blender --background --python tools/blender/src/strip_animation.py -- \
        --glb art/animations/idle.glb
"""

import argparse
import pathlib
import sys
import traceback

import bpy


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--glb", type=pathlib.Path, required=True)
    return parser.parse_args(argv)


def strip(path: pathlib.Path) -> None:
    """Rewrites `path` in place with only its armature and action."""
    if not path.exists():
        sys.exit(f"error: {path} not found")

    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=str(path))

    armature = next((o for o in bpy.data.objects if o.type == "ARMATURE"), None)
    if armature is None:
        sys.exit(f"error: {path} has no armature, so it drives nothing")
    if not bpy.data.actions:
        sys.exit(f"error: {path} contains no animation")

    for obj in list(bpy.data.objects):
        if obj is not armature:
            bpy.data.objects.remove(obj, do_unlink=True)
    for image in list(bpy.data.images):
        bpy.data.images.remove(image)
    for material in list(bpy.data.materials):
        bpy.data.materials.remove(material)

    carrier = skin_carrier(armature)

    bpy.ops.object.select_all(action="DESELECT")
    armature.select_set(True)
    carrier.select_set(True)
    bpy.context.view_layer.objects.active = armature
    bpy.ops.export_scene.gltf(
        filepath=str(path),
        export_format="GLB",
        use_selection=True,
        export_animations=True,
        export_skins=True,
        export_materials="NONE",
    )


def skin_carrier(armature: bpy.types.Object) -> bpy.types.Object:
    """A tiny triangle weighted to the root bone, so the armature exports."""
    root = next(b for b in armature.data.bones if b.parent is None)
    mesh = bpy.data.meshes.new("skin_carrier")
    mesh.from_pydata(
        [(0.0, 0.0, 0.0), (0.001, 0.0, 0.0), (0.0, 0.001, 0.0)], [], [(0, 1, 2)]
    )
    mesh.update()

    carrier = bpy.data.objects.new("skin_carrier", mesh)
    bpy.context.collection.objects.link(carrier)
    group = carrier.vertex_groups.new(name=root.name)
    group.add([0, 1, 2], 1.0, "REPLACE")
    carrier.parent = armature
    modifier = carrier.modifiers.new("Armature", "ARMATURE")
    modifier.object = armature
    return carrier


def main() -> None:
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    args = parse_args(argv)
    before = args.glb.stat().st_size if args.glb.exists() else 0
    strip(args.glb)
    after = args.glb.stat().st_size
    print(f"stripped {args.glb.name}: {before / 1e6:.2f} MB -> {after / 1e3:.0f} KB")


if __name__ == "__main__":
    # Blender exits 0 on an uncaught exception, so a failure here would look
    # like success and leave a multi-megabyte file committed.
    try:
        main()
    except Exception:  # noqa: BLE001
        traceback.print_exc()
        sys.exit(1)
