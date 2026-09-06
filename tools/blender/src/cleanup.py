"""What the mesh fixer does, and in which order.

The decisions `mesh_clean.py` makes without `bpy`: the order, which pieces
are debris, which objects survive, and the line the run prints. Every size
arrives on argv, so nothing here is a number.
"""

from collections.abc import Iterable, Sequence

Vec3 = tuple[float, float, float]
Triangle = tuple[Vec3, Vec3, Vec3]

WORLD = "world"
"""Take every mesh to world space. First, and not optional: fact 13 is
Meshy's own extension deleting nothing because it measured piece volume in
local space while its checker measured in world space, and this asset family
carries a 100x node scale."""

WELD = "weld"
"""Merge coincident vertices. glTF splits one at every UV seam."""

ALLOWLIST = "allowlist"
"""Drop every object `[profile] meshes` does not name."""

ISLANDS = "islands"
"""Drop pieces under `[profile.cleanup] smallest_island_cubic_meters`."""

HOLES = "holes"
"""Fill what boundary edges are left, once the debris is gone."""

SYMMETRIZE = "symmetrize"
"""Mirror the mesh about +X, when the spec asks for it."""


def ordered_steps(symmetry: bool) -> tuple[str, ...]:
    """The fixer's steps, in the one order that works.

    World space first, then the weld, because an unwelded mesh reads 13,368
    boundary edges where it has 171. Debris before the holes, because a hole
    in a piece about to be deleted is not worth closing.
    """
    steps = (WORLD, WELD, ALLOWLIST, ISLANDS, HOLES)
    return (*steps, SYMMETRIZE) if symmetry else steps


def island_volume(triangles: Iterable[Triangle]) -> float:
    """How much space one connected piece encloses, in cubic meters.

    The signed tetrahedron sum about the piece's own center, so a piece far
    from the origin is not read as an enormous one. Closed pieces are what
    this is for; an open one gives the size of the volume it bounds.
    """
    faces = list(triangles)
    if not faces:
        return 0.0
    corners = [corner for face in faces for corner in face]
    middle = tuple(sum(axis) / len(corners) for axis in zip(*corners, strict=True))
    total = 0.0
    for face in faces:
        a, b, c = (
            tuple(corner[axis] - middle[axis] for axis in range(3)) for corner in face
        )
        cross = (
            b[1] * c[2] - b[2] * c[1],
            b[2] * c[0] - b[0] * c[2],
            b[0] * c[1] - b[1] * c[0],
        )
        total += sum(a[axis] * cross[axis] for axis in range(3)) / 6.0
    return abs(total)


def dropped_islands(volumes: Sequence[float], floor: float) -> tuple[int, ...]:
    """Which pieces are debris: the ones under `floor`.

    Under, not at: a piece exactly the size of the floor is kept, the same way
    `mesh.cleanup_effective` is `lt` and not `le`.
    """
    dropped = tuple(at for at, volume in enumerate(volumes) if volume < floor)
    if volumes and len(dropped) == len(volumes):
        raise ValueError(
            f"the island floor of {floor} m3 drops every one of the "
            f"{len(volumes)} pieces, which would export an empty mesh"
        )
    return dropped


def kept_objects(names: Sequence[str], allowed: Sequence[str]) -> tuple[str, ...]:
    """The objects the profile allows, of the ones the file holds."""
    kept = tuple(name for name in names if name in set(allowed))
    if names and not kept:
        raise ValueError(
            f"the profile allows {sorted(allowed)}, which is none of the "
            f"{len(names)} objects in this file: {sorted(names)}"
        )
    return kept


def summary(
    *,
    objects: Sequence[str],
    dropped_objects: Sequence[str],
    dropped_islands: int,
    vertices_before: int,
    vertices_after: int,
    images: int,
    uv_layers: int,
    symmetry: bool,
) -> str:
    """One line saying what the run did, as `key=value` pairs.

    A log line and not a finding: `mesh.texture` and `mesh.uv` are the gates,
    in Rust, on the file the fixer wrote.
    """
    return " ".join(
        [
            f"cleaned={','.join(objects)}",
            f"dropped_objects={','.join(dropped_objects) or 'none'}",
            f"dropped_islands={dropped_islands}",
            f"vertices_in={vertices_before}",
            f"vertices_out={vertices_after}",
            f"images={images}",
            f"uv_layers={uv_layers}",
            f"mirrored={symmetry}",
        ]
    )
