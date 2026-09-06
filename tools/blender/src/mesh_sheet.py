"""Render one bare mesh into the views the model contact sheet is made of.

Blender glue only: `framing.py` decides where every camera goes. **It measures
nothing.** Acceptance item 4 of the `pose_mode` spike is a human reading the
sheet, so what this writes is pixels, and `crates/xtask-art/src/spike.rs`
composites them.
"""

import argparse
import pathlib
import sys

import bpy
from bake_sprites import setup_lighting, setup_render
from findings import guard
from framing import Bounds, Still, Vec3, mesh_stills
from mathutils import Vector


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--glb", type=pathlib.Path, required=True)
    parser.add_argument(
        "--out-dir",
        type=pathlib.Path,
        required=True,
        help="Where one PNG per view is written, named after the view.",
    )
    parser.add_argument(
        "--size", type=int, required=True, help="Render size in pixels."
    )
    return parser.parse_args(argv)


def world_points(meshes: list[bpy.types.Object]) -> list[Vec3]:
    """Every vertex in world space. The node scale is 0.01 over coordinates
    100x larger on this asset family, so local space is not the same shape."""
    return [
        tuple(obj.matrix_world @ vertex.co)
        for obj in meshes
        for vertex in obj.data.vertices
    ]


def render(still: Still, out_dir: pathlib.Path) -> pathlib.Path:
    """One view, from a camera and a key light placed for it alone."""
    scene = bpy.context.scene
    camera = scene.camera
    camera.data.ortho_scale = still.ortho_scale
    camera.location = Vector(still.camera_location)
    camera.rotation_euler = still.camera_rotation
    for light in (o for o in bpy.data.objects if o.type == "LIGHT"):
        light.rotation_euler = still.key_light_rotation

    path = out_dir / f"{still.name}.png"
    scene.render.filepath = str(path)
    bpy.ops.render.render(write_still=True)
    return path


def draw(args: argparse.Namespace) -> None:
    if not args.glb.exists():
        sys.exit(f"error: {args.glb} not found, run the model stage first")

    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=str(args.glb))
    meshes = [o for o in bpy.context.scene.objects if o.type == "MESH"]
    if not meshes:
        sys.exit(f"error: {args.glb} holds no mesh, so there is nothing to draw")

    points = world_points(meshes)
    body = Bounds(
        lo=tuple(min(point[axis] for point in points) for axis in range(3)),
        hi=tuple(max(point[axis] for point in points) for axis in range(3)),
    )
    setup_render(args.size)
    setup_lighting()
    camera_data = bpy.data.cameras.new("still_cam")
    camera_data.type = "ORTHO"
    camera = bpy.data.objects.new("still_cam", camera_data)
    bpy.context.collection.objects.link(camera)
    bpy.context.scene.camera = camera

    args.out_dir.mkdir(parents=True, exist_ok=True)
    drawn = [render(still, args.out_dir).name for still in mesh_stills(body, points)]
    print(f"drew={','.join(drawn)} vertices={len(points)}")


def main() -> None:
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    draw(parse_args(argv))


def save_blend(path: pathlib.Path) -> None:
    """The scene as it stood when the script failed, for a human to open."""
    bpy.ops.wm.save_as_mainfile(filepath=str(path), copy=True)


if __name__ == "__main__":
    # Blender exits 0 on an uncaught exception, so a sheet that was never
    # drawn would look like one that was. `guard` writes the success sentinel
    # the Rust side asserts.
    guard(main, save_blend)
