"""Clean the bare mesh, before a single rigging credit is spent.

Blender glue only: `cleanup.py` decides, this runs it on the unskinned mesh.
**It measures nothing**, so `crates/xtask-art/src/check/mesh.rs` reads the
mesh it was given beside the mesh it wrote. `tools/blender/README.md` has why.
"""

import argparse
import pathlib
import sys
from collections.abc import Callable

import bmesh
import bpy
import cleanup
from findings import guard
from mathutils import Matrix


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--glb", type=pathlib.Path, required=True)
    parser.add_argument("--out", type=pathlib.Path, required=True)
    parser.add_argument(
        "--meshes",
        required=True,
        help="The objects `[profile] meshes` allows, comma separated. "
        "Everything else in the file is debris and is dropped.",
    )
    parser.add_argument(
        "--weld",
        type=float,
        required=True,
        help="How near two vertices must be to be merged, in world meters. "
        "The same distance `check/gltf_mesh.rs` measures at.",
    )
    parser.add_argument(
        "--island-volume",
        type=float,
        required=True,
        help="A connected piece under this many cubic meters is debris.",
    )
    parser.add_argument(
        "--symmetry-threshold",
        type=float,
        required=True,
        help="How far a vertex may sit from its reflection and still be "
        "mirrored onto it, in meters.",
    )
    parser.add_argument(
        "--symmetry",
        required=True,
        choices=["true", "false"],
        help="What `spec.subject.symmetry` declares for this character.",
    )
    return parser.parse_args(argv)


class Fixer:
    """One mesh file, and the steps `cleanup.ordered_steps` puts in order."""

    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.objects = [o for o in bpy.context.scene.objects if o.type == "MESH"]
        self.dropped_objects: tuple[str, ...] = ()
        self.dropped_islands = 0
        self.vertices_before = sum(len(o.data.vertices) for o in self.objects)

    def steps(self) -> dict[str, Callable[[], None]]:
        return {
            cleanup.WORLD: self.to_world,
            cleanup.WELD: self.weld,
            cleanup.ALLOWLIST: self.allowlist,
            cleanup.ISLANDS: self.drop_debris,
            cleanup.HOLES: self.fill_holes,
            cleanup.SYMMETRIZE: self.symmetrize,
        }

    def run(self) -> None:
        steps = self.steps()
        for step in cleanup.ordered_steps(self.args.symmetry == "true"):
            steps[step]()

    def to_world(self) -> None:
        """World space, once, up front. Fact 13 is what happens otherwise."""
        for obj in self.objects:
            self.edit(obj, lambda bm, obj=obj: bm.transform(obj.matrix_world))
            obj.matrix_world = Matrix.Identity(4)

    def weld(self) -> None:
        for obj in self.objects:
            self.edit(
                obj,
                lambda bm: bmesh.ops.remove_doubles(
                    bm, verts=bm.verts, dist=self.args.weld
                ),
            )

    def allowlist(self) -> None:
        names = [obj.name for obj in self.objects]
        kept = cleanup.kept_objects(names, self.args.meshes.split(","))
        self.dropped_objects = tuple(name for name in names if name not in kept)
        for obj in list(self.objects):
            if obj.name in self.dropped_objects:
                self.objects.remove(obj)
                bpy.data.objects.remove(obj, do_unlink=True)

    def drop_debris(self) -> None:
        for obj in self.objects:
            self.edit(obj, self.drop_small_pieces)

    def drop_small_pieces(self, bm: bmesh.types.BMesh) -> None:
        pieces = islands_of(bm)
        volumes = [cleanup.island_volume(triangles_of(piece)) for piece in pieces]
        dropped = cleanup.dropped_islands(volumes, self.args.island_volume)
        self.dropped_islands += len(dropped)
        debris = [face for at in dropped for face in pieces[at]]
        if debris:
            bmesh.ops.delete(bm, geom=debris, context="FACES")

    def fill_holes(self) -> None:
        for obj in self.objects:
            self.edit(
                obj,
                # sides=0 fills a boundary loop of any length. The default of
                # 4 leaves every larger hole open.
                lambda bm: bmesh.ops.holes_fill(bm, edges=bm.edges[:], sides=0),
            )

    def symmetrize(self) -> None:
        for obj in self.objects:
            self.edit(
                obj,
                lambda bm: bmesh.ops.symmetrize(
                    bm,
                    input=bm.verts[:] + bm.edges[:] + bm.faces[:],
                    # "X" keeps the +X half and mirrors it onto -X, which is
                    # what the operator spells POSITIVE_X. The operator's own
                    # spelling is not accepted here: bmesh takes -X or X.
                    direction="X",
                    dist=self.args.symmetry_threshold,
                ),
            )

    @staticmethod
    def edit(obj: bpy.types.Object, change: Callable[..., object]) -> None:
        """One bmesh in, one bmesh out. Each step reads the mesh the step
        before it wrote, so nothing operates on a stale copy."""
        bm = bmesh.new()
        bm.from_mesh(obj.data)
        change(bm)
        bm.to_mesh(obj.data)
        obj.data.update()
        bm.free()

    def said(self) -> str:
        return cleanup.summary(
            objects=tuple(obj.name for obj in self.objects),
            dropped_objects=self.dropped_objects,
            dropped_islands=self.dropped_islands,
            vertices_before=self.vertices_before,
            vertices_after=sum(len(o.data.vertices) for o in self.objects),
            images=len(bpy.data.images),
            uv_layers=sum(len(o.data.uv_layers) for o in self.objects),
            symmetry=self.args.symmetry == "true",
        )


def islands_of(bm: bmesh.types.BMesh) -> list[list[bmesh.types.BMFace]]:
    """The connected pieces of one mesh, over faces that share a vertex.

    The same connectivity `check/gltf_mesh.rs::islands` counts, so a piece
    this drops is a piece that gate was counting.
    """
    # Without this the indices can be stale, and every face would look like
    # face -1: one island, and nothing dropped.
    bm.faces.index_update()
    seen: set[int] = set()
    pieces = []
    for start in bm.faces:
        if start.index in seen:
            continue
        seen.add(start.index)
        stack, piece = [start], []
        while stack:
            face = stack.pop()
            piece.append(face)
            for vert in face.verts:
                for other in vert.link_faces:
                    if other.index not in seen:
                        seen.add(other.index)
                        stack.append(other)
        pieces.append(piece)
    return pieces


def triangles_of(faces: list[bmesh.types.BMFace]) -> list[cleanup.Triangle]:
    """One piece as triangles, fanned from each face's first corner."""
    fanned = []
    for face in faces:
        corners = [tuple(vert.co) for vert in face.verts]
        for at in range(1, len(corners) - 1):
            fanned.append((corners[0], corners[at], corners[at + 1]))
    return fanned


def clean(args: argparse.Namespace) -> None:
    if not args.glb.exists():
        sys.exit(f"error: {args.glb} not found, run the model stage first")

    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=str(args.glb))
    fixer = Fixer(args)
    if not fixer.objects:
        sys.exit(f"error: {args.glb} holds no mesh, so there is nothing to clean")
    fixer.run()
    export(fixer.objects, args.out)
    print(fixer.said())


def export(objects: list[bpy.types.Object], out: pathlib.Path) -> None:
    """The cleaned meshes, and nothing else in the scene."""
    out.parent.mkdir(parents=True, exist_ok=True)
    bpy.ops.object.select_all(action="DESELECT")
    for obj in objects:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = objects[0]
    bpy.ops.export_scene.gltf(
        filepath=str(out),
        export_format="GLB",
        use_selection=True,
        export_animations=False,
        # No armature reaches this file, which is why it is the one export
        # here that does not ask for a rest position.
        export_skins=False,
        # Rigging refuses an untextured mesh, so the texture and the
        # coordinates that sample it are the point of the export.
        export_materials="EXPORT",
        export_texcoords=True,
    )


def main() -> None:
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    clean(parse_args(argv))


def save_blend(path: pathlib.Path) -> None:
    """The scene as it stood when the script failed, for a human to open."""
    bpy.ops.wm.save_as_mainfile(filepath=str(path), copy=True)


if __name__ == "__main__":
    # Blender exits 0 on an uncaught exception, so a mesh that was never
    # cleaned would look like one that was. `guard` writes the success
    # sentinel the Rust side asserts.
    guard(main, save_blend)
