"""The decisions the mesh fixer makes, with no Blender in sight.

`mesh_clean.py` is the shell that owns `bpy` and `bmesh`. Everything that can
be decided without them is here: the order of the steps, what the two flags
switch off, which islands are debris, and what the run says it did.
"""

import pytest
from cleanup import (
    ALLOWLIST,
    HOLES,
    ISLANDS,
    SYMMETRIZE,
    WELD,
    WORLD,
    dropped_islands,
    island_volume,
    kept_objects,
    ordered_steps,
    summary,
)

CENTIMETER_CUBE = 0.01
"""The side of the cube the island floor is published against."""


def cube(side: float, at: tuple[float, float, float] = (0.0, 0.0, 0.0)) -> tuple:
    """A closed box as triangles, in world meters."""
    x, y, z = at
    corners = [
        (x + sx * side / 2, y + sy * side / 2, z + sz * side / 2)
        for sx, sy, sz in (
            (-1, -1, -1),
            (1, -1, -1),
            (1, -1, 1),
            (-1, -1, 1),
            (-1, 1, -1),
            (1, 1, -1),
            (1, 1, 1),
            (-1, 1, 1),
        )
    ]
    faces = [
        (0, 2, 1),
        (0, 3, 2),
        (4, 5, 6),
        (4, 6, 7),
        (0, 1, 5),
        (0, 5, 4),
        (2, 3, 7),
        (2, 7, 6),
        (1, 2, 6),
        (1, 6, 5),
        (3, 0, 4),
        (3, 4, 7),
    ]
    return tuple(tuple(corners[i] for i in face) for face in faces)


# --- the order the steps run in -------------------------------------------


def test_the_world_transform_comes_before_anything_that_measures_a_size():
    """Fact 13: Meshy's own extension deleted nothing because it measured
    piece volume in local space while its checker measured world space."""
    steps = ordered_steps(symmetry=True)

    assert steps[0] == WORLD
    assert steps.index(WORLD) < steps.index(ISLANDS)


def test_the_weld_comes_before_the_holes_are_filled():
    """glTF splits a vertex at every UV seam, so an unwelded mesh reads
    13,368 boundary edges where it has 171. Filling those would lay 13,000
    faces over a surface that is already closed."""
    steps = ordered_steps(symmetry=True)

    assert steps.index(WELD) < steps.index(HOLES)


def test_debris_is_dropped_before_the_holes_are_filled():
    """A hole in a piece that is about to be deleted is not worth closing."""
    steps = ordered_steps(symmetry=True)

    assert steps.index(ALLOWLIST) < steps.index(ISLANDS) < steps.index(HOLES)


def test_the_mirror_is_the_last_step_and_only_when_the_spec_asks():
    with_it = ordered_steps(symmetry=True)
    without = ordered_steps(symmetry=False)

    assert with_it[-1] == SYMMETRIZE
    assert SYMMETRIZE not in without
    assert without == with_it[:-1], "and nothing else moves"


def test_every_step_is_named_once():
    steps = ordered_steps(symmetry=True)

    assert len(set(steps)) == len(steps)


# --- which pieces are debris ----------------------------------------------


def test_a_one_centimeter_cube_measures_exactly_the_published_floor():
    """`[profile.cleanup] smallest_island_cubic_meters` says "about a 1 cm
    cube", and this is the measurement behind that sentence."""
    assert island_volume(cube(CENTIMETER_CUBE)) == pytest.approx(1e-6, rel=1e-9)


def test_the_volume_does_not_depend_on_where_the_piece_sits():
    """A piece 10 m from the origin is the same size as one at it. Measured
    about the piece's own center, so a far-away island is not read as huge."""
    near = island_volume(cube(CENTIMETER_CUBE))
    far = island_volume(cube(CENTIMETER_CUBE, at=(10.0, -4.0, 7.0)))

    assert far == pytest.approx(near, rel=1e-6)


def test_a_piece_with_no_face_has_no_volume():
    assert island_volume(()) == 0.0


def test_only_the_pieces_under_the_floor_are_dropped():
    volumes = [1e-3, 1e-7, 1e-6, 1e-9]

    assert dropped_islands(volumes, floor=1e-6) == (1, 3)


def test_a_piece_exactly_at_the_floor_is_kept():
    """The floor is what a piece must be under, so equality keeps it: this is
    the same `lt` versus `le` the no-op fixer stub turns on."""
    assert dropped_islands([1e-6], floor=1e-6) == ()


def test_dropping_every_piece_is_refused_rather_than_writing_an_empty_mesh():
    """An empty export passes `mesh.non_manifold_post` at zero edges. The
    fixer stops instead, and the run leaves no sentinel."""
    with pytest.raises(ValueError, match="every one of the 2 pieces"):
        dropped_islands([1e-9, 1e-8], floor=1e-6)


# --- which objects survive -------------------------------------------------


def test_only_the_objects_the_profile_allows_are_kept():
    kept = kept_objects(["char1", "Icosphere", "debris"], allowed=["char1"])

    assert kept == ("char1",)


def test_an_allowlist_that_matches_nothing_is_refused():
    """Every object dropped is the same empty export as every island
    dropped, and it is the more likely typo of the two."""
    with pytest.raises(ValueError, match="none of the 2 objects"):
        kept_objects(["char1", "Icosphere"], allowed=["body"])


# --- what the run says it did ----------------------------------------------


def test_the_summary_is_key_equals_value_and_names_the_texture_it_wrote():
    """The fixer measures nothing, so this is a line to read and not a
    finding. `mesh.texture` and `mesh.uv` are the gates, in Rust, on the file
    it wrote."""
    said = summary(
        objects=("char1",),
        dropped_objects=("Icosphere",),
        dropped_islands=6,
        vertices_before=34506,
        vertices_after=27508,
        images=1,
        uv_layers=1,
        symmetry=True,
    )

    assert said == (
        "cleaned=char1 dropped_objects=Icosphere dropped_islands=6 "
        "vertices_in=34506 vertices_out=27508 images=1 uv_layers=1 "
        "mirrored=True"
    )
    assert "dropped_objects=none" in summary(
        objects=("char1",),
        dropped_objects=(),
        dropped_islands=0,
        vertices_before=8,
        vertices_after=8,
        images=1,
        uv_layers=1,
        symmetry=False,
    )
