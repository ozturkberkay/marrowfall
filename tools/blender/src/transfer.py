"""The retarget maths: world matrices in, local poses out.

A clip authored on someone else's skeleton is fitted by transferring **world**
orientations, never local ones. Every rig rolls its bones differently, and a
local rotation means nothing outside the rest pose it was authored against,
which is how the code this replaces left both wrists 53 and 67 degrees out of
their own rest.

The whole method is four steps:

1. Aim both rigs at the same absolute table of world directions, one row per
   role, keeping each rig's own roll. That pose is the **reference pose**, and
   it is recorded per rig as `ref_world_ours` and `ref_world_src`.
2. `Offset(role) = ref_world_src(role)^-1 @ ref_world_ours(role)`, one constant
   rotation per role. Nothing here ever sees the aim table again: feeding the
   table where a reference pose belongs would make the offset a re-roll rather
   than a correction.
3. `world_out(role, t) = world_src(role, t) @ Offset(role)`, per frame.
4. Recover each local pose algebraically, parents first, from the rest
   hierarchy. Nothing is read back out of a posed target, so the dependency
   graph is evaluated once per source frame rather than once per bone per
   frame.

**Only the root gets a location.** `world_out` carries the source's joint
position, so writing it to every bone drags our joints onto the source's and
our rig inherits its limb lengths.

Free of `bpy`, so it is unit tested with no Blender. Blender's own
`mathutils` is not importable outside Blender and is not a dependency of this
repository, so the small amount of linear algebra this needs lives here.
"""

import math

from framing import Frozen, Vec3

Row = tuple[float, float, float, float]
Mat4 = tuple[Row, Row, Row, Row]
"""A 4x4 affine transform, row major, the way `mathutils.Matrix` indexes."""

Quat = tuple[float, float, float, float]
"""A rotation, in Blender's w, x, y, z order."""

CHILD_AXIS: Vec3 = (0.0, 1.0, 0.0)
"""The axis a bone points along. Blender's own convention, not a rig's
choice: every bone runs from its head to its tail along local +Y."""

IDENTITY: Mat4 = (
    (1.0, 0.0, 0.0, 0.0),
    (0.0, 1.0, 0.0, 0.0),
    (0.0, 0.0, 1.0, 0.0),
    (0.0, 0.0, 0.0, 1.0),
)

NO_ROTATION: Quat = (1.0, 0.0, 0.0, 0.0)

# What counts as nothing here. On a unit quaternion a `w` this small is a
# rotation within 1e-7 degrees of half a turn, and on a direction it is a
# length of 1e-9 in the rig's own units. Both are noise rather than a
# measurement, and the tightest limit any gate publishes is 1.0 degree.
EPSILON = 1e-9


class TransferError(Exception):
    """A refusal with a code and the bone or role it is about.

    Typed rather than printed: the retarget runs unattended, and a message on
    stdout is how a broken fit shipped as a success. The codes:

    - `role_unmapped`: a role one side has and the other does not, or a chain
      that never reaches a top.
    - `aim_row_missing`: a mapped role with no row in the aim table. Never
      filled from the source's own rest pose, because a silent fallback is
      what left seven bones uncorrected.
    - `bone_missing`: a bone whose parent is not in the rig.
    - `swing_singular`: half a turn of swing, where the twist does not exist.
    - `degenerate_basis`: a matrix that is not an invertible right handed
      frame, so it holds no rotation to read.
    - `no_direction`: a vector or a quaternion with no length.
    """

    def __init__(self, code: str, subject: str) -> None:
        super().__init__(f"{code}: {subject}")
        self.code = code
        self.subject = subject


class Bone(Frozen):
    """One bone of the target rig, as the transfer needs it."""

    name: str
    parent: str | None
    """The real bone hierarchy, which a local pose composes along."""
    role: str | None
    """The anatomical role, or None for a bone no convention maps."""
    rest: Mat4
    """`bone.matrix_local`: the rest transform in ARMATURE space."""


class LocalPose(Frozen):
    """What one bone is keyed to, in its own basis space.

    `location` is set for the root alone, per the module docstring.
    """

    rotation: Quat
    location: Vec3 | None = None


# --- vectors ---------------------------------------------------------------


def dot(a: Vec3, b: Vec3) -> float:
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def cross(a: Vec3, b: Vec3) -> Vec3:
    return (
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    )


def length(v: Vec3) -> float:
    return math.sqrt(dot(v, v))


def normalized(v: Vec3) -> Vec3:
    if (size := length(v)) < EPSILON:
        raise TransferError(code="no_direction", subject=str(v))
    return (v[0] / size, v[1] / size, v[2] / size)


def scaled(v: Vec3, factor: float) -> Vec3:
    return (v[0] * factor, v[1] * factor, v[2] * factor)


# --- quaternions -----------------------------------------------------------


def quat_multiply(a: Quat, b: Quat) -> Quat:
    """`a @ b`: b applied first, then a."""
    aw, ax, ay, az = a
    bw, bx, by, bz = b
    return (
        aw * bw - ax * bx - ay * by - az * bz,
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
    )


def quat_inverted(q: Quat) -> Quat:
    """The conjugate, which inverts a unit quaternion."""
    return (q[0], -q[1], -q[2], -q[3])


def quat_normalized(q: Quat) -> Quat:
    size = math.sqrt(sum(part * part for part in q))
    if size < EPSILON:
        raise TransferError(code="no_direction", subject=str(q))
    return (q[0] / size, q[1] / size, q[2] / size, q[3] / size)


def quat_degrees(q: Quat) -> float:
    """How far a rotation turns, in degrees, ignoring its direction.

    A quaternion and its negation are the same rotation, hence the absolute
    value: without it, half of these read as a full turn.
    """
    return math.degrees(2.0 * math.acos(min(1.0, abs(q[0]))))


def quat_from_axis(axis: Vec3, degrees: float) -> Quat:
    """A rotation of `degrees` about `axis`."""
    half = math.radians(degrees) / 2.0
    turn = scaled(normalized(axis), math.sin(half))
    return (math.cos(half), turn[0], turn[1], turn[2])


def swing_twist(rotation: Quat, axis: Vec3, subject: str) -> tuple[Quat, Quat]:
    """Split `rotation` into a swing and a twist about `axis`.

    `rotation == swing @ twist`, so the twist is applied first, in the bone's
    own frame, and the swing then points the bone.

    Projects the quaternion's VECTOR PART, never `mathutils`'
    `Quaternion.axis`, which is normalized and so drops the `sin(angle / 2)`
    factor. On a pure 10 degree twist the wrong form reads twist 90.22 and
    swing 80.22 against a true 10.00 and 0.00, and it is worst NEAR IDENTITY,
    where a pure swing hides it entirely
    (`docs/research/agent_reports/proof_swing_twist_vector_part.md`).
    """
    unit = normalized(axis)
    projected = scaled(unit, dot((rotation[1], rotation[2], rotation[3]), unit))
    if abs(rotation[0]) < EPSILON and length(projected) < EPSILON:
        # Half a turn about an axis square to `axis`. The twist is not small
        # here, it does not exist: every twist gives the same swing.
        raise TransferError(code="swing_singular", subject=subject)
    twist = quat_normalized((rotation[0], projected[0], projected[1], projected[2]))
    return quat_multiply(rotation, quat_inverted(twist)), twist


def aim_rotation(source: Vec3, target: Vec3) -> Quat:
    """The shortest rotation that turns `source` onto `target`.

    Shortest, so it adds no twist about either direction. Half a turn has no
    shortest arc, and that case is answered here with a well defined axis
    square to `source` so the one caller that cannot proceed refuses in
    `swing_twist` rather than dividing by zero here.
    """
    from_unit, to_unit = normalized(source), normalized(target)
    axis = cross(from_unit, to_unit)
    if length(axis) < EPSILON:
        if dot(from_unit, to_unit) > 0.0:
            return NO_ROTATION
        return quat_from_axis(_square_to(from_unit), 180.0)
    return quat_normalized((1.0 + dot(from_unit, to_unit), axis[0], axis[1], axis[2]))


def _square_to(v: Vec3) -> Vec3:
    """Some unit direction at a right angle to `v`.

    Crossed with whichever world axis `v` leans on least, so the result is
    never degenerate.
    """
    least = min(range(3), key=lambda index: abs(v[index]))
    axis: Vec3 = (float(least == 0), float(least == 1), float(least == 2))
    return normalized(cross(v, axis))


# --- matrices --------------------------------------------------------------


def mat_multiply(a: Mat4, b: Mat4) -> Mat4:
    return (
        _combined(a[0], b),
        _combined(a[1], b),
        _combined(a[2], b),
        _combined(a[3], b),
    )


def _combined(row: Row, b: Mat4) -> Row:
    """One row of `a @ b`."""
    out = [sum(row[k] * b[k][col] for k in range(4)) for col in range(4)]
    return (out[0], out[1], out[2], out[3])


def mat_inverted(m: Mat4, subject: str) -> Mat4:
    """The inverse of an affine transform, whose last row is 0, 0, 0, 1.

    Every matrix here comes from `matrix_world @ pose.matrix` or from
    `bone.matrix_local`, and both are affine. The rotation and scale part is
    inverted through its adjugate, and the translation follows it.
    """
    (a, b, c, _), (d, e, f, _), (g, h, i, _) = m[0], m[1], m[2]
    adjugate = (
        (e * i - f * h, c * h - b * i, b * f - c * e),
        (f * g - d * i, a * i - c * g, c * d - a * f),
        (d * h - e * g, b * g - a * h, a * e - b * d),
    )
    determinant = a * adjugate[0][0] + b * adjugate[1][0] + c * adjugate[2][0]
    # Against the volume the three columns could enclose, not against a fixed
    # number: a rig scaled by 1e-3 has a determinant of 1e-9 and is perfectly
    # invertible, while a flattened one has a determinant of 0 whatever its
    # size. The ratio is 1 for any orthogonal basis and 0 for a flat one.
    volume = length((a, d, g)) * length((b, e, h)) * length((c, f, i))
    if volume < EPSILON or abs(determinant) < EPSILON * volume:
        raise TransferError(code="degenerate_basis", subject=subject)
    basis = [[value / determinant for value in row] for row in adjugate]
    offset = [-sum(basis[row][k] * m[k][3] for k in range(3)) for row in range(3)]
    return (
        (basis[0][0], basis[0][1], basis[0][2], offset[0]),
        (basis[1][0], basis[1][1], basis[1][2], offset[1]),
        (basis[2][0], basis[2][1], basis[2][2], offset[2]),
        (0.0, 0.0, 0.0, 1.0),
    )


def mat_direction(m: Mat4, v: Vec3) -> Vec3:
    """`v` carried by `m`'s rotation and scale, with no translation."""
    out = [sum(m[row][k] * v[k] for k in range(3)) for row in range(3)]
    return (out[0], out[1], out[2])


def mat_translation(m: Mat4) -> Vec3:
    return (m[0][3], m[1][3], m[2][3])


def mat_rotation(m: Mat4, subject: str) -> Quat:
    """`m`'s rotation, with any scale divided out first.

    Shepperd's method: whichever of the four components is largest is solved
    for directly, so no near-zero divisor is ever used. A left-handed frame
    has no rotation at all and is refused rather than turned into a plausible
    quaternion.
    """
    r = _orthonormal(m, subject)
    trace = r[0][0] + r[1][1] + r[2][2]
    if trace > 0.0:
        s = 2.0 * math.sqrt(1.0 + trace)
        return (
            0.25 * s,
            (r[2][1] - r[1][2]) / s,
            (r[0][2] - r[2][0]) / s,
            (r[1][0] - r[0][1]) / s,
        )
    if r[0][0] >= r[1][1] and r[0][0] >= r[2][2]:
        s = 2.0 * math.sqrt(1.0 + r[0][0] - r[1][1] - r[2][2])
        return (
            (r[2][1] - r[1][2]) / s,
            0.25 * s,
            (r[0][1] + r[1][0]) / s,
            (r[0][2] + r[2][0]) / s,
        )
    if r[1][1] >= r[2][2]:
        s = 2.0 * math.sqrt(1.0 + r[1][1] - r[0][0] - r[2][2])
        return (
            (r[0][2] - r[2][0]) / s,
            (r[0][1] + r[1][0]) / s,
            0.25 * s,
            (r[1][2] + r[2][1]) / s,
        )
    s = 2.0 * math.sqrt(1.0 + r[2][2] - r[0][0] - r[1][1])
    return (
        (r[1][0] - r[0][1]) / s,
        (r[0][2] + r[2][0]) / s,
        (r[1][2] + r[2][1]) / s,
        0.25 * s,
    )


def _orthonormal(m: Mat4, subject: str) -> list[Vec3]:
    """`m`'s upper 3x3 as rows, scale removed, refused if left-handed."""
    columns = [normalized((m[0][axis], m[1][axis], m[2][axis])) for axis in range(3)]
    if dot(cross(columns[0], columns[1]), columns[2]) <= 0.0:
        raise TransferError(code="degenerate_basis", subject=subject)
    return [(columns[0][row], columns[1][row], columns[2][row]) for row in range(3)]


# --- the transfer ----------------------------------------------------------


def reference_pose(
    rest_world: dict[str, Mat4], chain: dict[str, str], aim: dict[str, Vec3]
) -> dict[str, Mat4]:
    """Where each role's bone lands when the aim table points it.

    Parents first, because a bone's world orientation is its parent's plus its
    own. Every mapped role is aimed, torso, hands and toes included: the code
    this replaces corrected only bones with exactly one mapped child, which is
    what left `Hips`, the spine, `Head`, both hands and both toes carrying
    this rig's own rest twist.

    `chain` is the retargeting hierarchy by role, already resolved so a role
    the source leaves out is stepped over. The result is one rig's half of the
    offset and is never mixed with the other's.
    """
    reference: dict[str, Mat4] = {}
    for role in _parents_first({r: chain.get(r) for r in rest_world}, "role_unmapped"):
        if (want := aim.get(role)) is None:
            raise TransferError(code="aim_row_missing", subject=role)
        rest = rest_world[role]
        if (parent := chain.get(role)) is None:
            carried = rest
        else:
            above = mat_inverted(rest_world[parent], parent)
            carried = mat_multiply(reference[parent], mat_multiply(above, rest))
        towards = mat_direction(mat_inverted(carried, role), want)
        # The swing alone: it is what points the bone, and dropping the twist
        # is what keeps this rig's own roll rather than inventing one.
        swing, _ = swing_twist(aim_rotation(CHILD_AXIS, towards), CHILD_AXIS, role)
        reference[role] = mat_multiply(carried, _quat_matrix(swing))
    return reference


def offsets(
    ref_world_ours: dict[str, Mat4], ref_world_src: dict[str, Mat4]
) -> dict[str, Quat]:
    """One constant rotation per role, from two reference poses.

    Takes reference poses and nothing else. Handed the aim table instead, it
    would return a re-roll of every bone onto the table's own frame, which
    reads plausible and is wrong on every joint.
    """
    if set(ref_world_ours) != set(ref_world_src):
        odd = set(ref_world_ours) ^ set(ref_world_src)
        raise TransferError(code="role_unmapped", subject=min(odd))
    return {
        role: quat_multiply(
            quat_inverted(mat_rotation(ref_world_src[role], role)),
            mat_rotation(ours, role),
        )
        for role, ours in ref_world_ours.items()
    }


def transfer(
    bones: tuple[Bone, ...],
    object_matrix: Mat4,
    world_src: dict[str, Mat4],
    offset: dict[str, Quat],
) -> dict[str, LocalPose]:
    """One frame of source world matrices, as local poses for our own rig.

    Returns only the bones this source drives. A bone with no role, or one
    whose role the source leaves out, is held at its rest transform so its
    driven children still compose from the right place.

    `object_matrix` is the target armature's own `matrix_world`, composed here
    and never applied to the rig: applying it to a rig that owns an action
    rescales the rest geometry and leaves the location keys byte identical, so
    their meaning silently changes by the object's scale.
    """
    if set(world_src) != set(offset):
        odd = set(world_src) ^ set(offset)
        raise TransferError(code="role_unmapped", subject=min(odd))
    by_name = {bone.name: bone for bone in bones}
    to_armature = mat_inverted(object_matrix, "the armature object")
    posed: dict[str, Mat4] = {}
    out: dict[str, LocalPose] = {}
    for name in _parents_first({b.name: b.parent for b in bones}, "bone_missing"):
        bone = by_name[name]
        parent = None if bone.parent is None else by_name[bone.parent]
        above = IDENTITY if parent is None else posed[parent.name]
        # The bone's rest, against its parent's: the space a key lives in.
        rest_basis = (
            bone.rest
            if parent is None
            else mat_multiply(mat_inverted(parent.rest, parent.name), bone.rest)
        )
        if bone.role is None or bone.role not in offset:
            posed[name] = (
                bone.rest if parent is None else mat_multiply(above, rest_basis)
            )
            continue
        world_out = mat_multiply(world_src[bone.role], _quat_matrix(offset[bone.role]))
        posed[name] = mat_multiply(to_armature, world_out)
        rested = mat_multiply(above, rest_basis)
        basis = mat_multiply(mat_inverted(rested, name), posed[name])
        out[name] = LocalPose(
            rotation=mat_rotation(basis, name),
            # A non-root bone keyed with a location would take our joint to
            # the source's, and our limb lengths with it.
            location=mat_translation(basis) if parent is None else None,
        )
    return out


def segment_length(rest_world: dict[str, Mat4], upper: str, lower: str) -> float:
    """The distance between two roles' joints, in the rest pose.

    The femur is the metric root travel is sized by: total height carries the
    head and the feet, and neither one takes a step.
    """
    for role in (upper, lower):
        if role not in rest_world:
            raise TransferError(code="role_unmapped", subject=role)
    head, tail = mat_translation(rest_world[upper]), mat_translation(rest_world[lower])
    return length((tail[0] - head[0], tail[1] - head[1], tail[2] - head[2]))


def _quat_matrix(q: Quat) -> Mat4:
    """A rotation as a 4x4, so it composes with the rest."""
    w, x, y, z = quat_normalized(q)
    return (
        (1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w), 0.0),
        (2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w), 0.0),
        (2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y), 0.0),
        (0.0, 0.0, 0.0, 1.0),
    )


def _parents_first(links: dict[str, str | None], code: str) -> list[str]:
    """Every key, each one after the parent it names.

    Order is not assumed of the caller: a child posed before its parent gives
    a plausible pose that is wrong everywhere below the first branch. A key
    whose parent is not here, and a cycle, both raise `code`.
    """
    ordered: list[str] = []
    placed: set[str] = set()
    remaining = set(links)
    while remaining:
        ready = sorted(
            key for key in remaining if links[key] is None or links[key] in placed
        )
        if not ready:
            raise TransferError(code=code, subject=min(remaining))
        ordered.extend(ready)
        placed.update(ready)
        remaining.difference_update(ready)
    return ordered
