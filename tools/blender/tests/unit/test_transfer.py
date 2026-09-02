"""The retarget maths.

Two things are asserted here and nothing else, because everything this module
replaced was checked against itself.

**Known answers, written as numbers a human computed once.** The swing-twist
split has two, and they are what tells the correct projection from the one
that reads plausible: this file carries the wrong form inline and pins the
90.22 and 80.22 degrees it produces.

**An absolute oracle.** Aiming both rigs at one table makes every offset a
pure twist about the bone's own axis, so `world_src @ Offset` points the bone
exactly where the source's bone points. The output's world bone directions
are therefore measured against the SOURCE's, never against the formula that
produced them, and each mutation below moves that number by a stated amount.
"""

import math

import pytest
from framing import Vec3
from transfer import (
    CHILD_AXIS,
    IDENTITY,
    NO_ROTATION,
    Bone,
    LocalPose,
    Mat4,
    Quat,
    TransferError,
    aim_rotation,
    cross,
    dot,
    length,
    mat_direction,
    mat_inverted,
    mat_multiply,
    mat_rotation,
    mat_translation,
    normalized,
    offsets,
    quat_degrees,
    quat_from_axis,
    quat_inverted,
    quat_multiply,
    quat_normalized,
    reference_pose,
    scaled,
    segment_length,
    swing_twist,
    transfer,
)

# --- fixtures --------------------------------------------------------------

# Both importers hand us an armature under a 0.01 object scale, and the bought
# one arrives turned as well. The transfer composes these and never applies
# them: applying the object transform to a rig that owns an action rescales
# the rest geometry and leaves every location key byte identical, so 2.316 m
# of travel silently becomes 231.599 m.
SOURCE_OBJECT: Mat4 = (
    (0.0090630778703665, -0.004226182617406995, 0.0, 0.3),
    (0.004226182617406995, 0.0090630778703665, 0.0, -0.2),
    (0.0, 0.0, 0.01, 0.0),
    (0.0, 0.0, 0.0, 1.0),
)
OUR_OBJECT: Mat4 = (
    (0.01, 0.0, 0.0, 0.0),
    (0.0, 0.01, 0.0, 0.0),
    (0.0, 0.0, 0.01, 0.0),
    (0.0, 0.0, 0.0, 1.0),
)

DOWN: Vec3 = (0.0, 0.0, -1.0)
UP: Vec3 = (0.0, 0.0, 1.0)
OUT_LEFT: Vec3 = (1.0, 0.0, -1.0)
OUT_RIGHT: Vec3 = (-1.0, 0.0, -1.0)
LEG: Vec3 = (0.0, 0.05, -1.0)
"""Where both rigs' legs point. Their legs agree on direction and disagree by
174 degrees of pure roll, which is what the committed rig and Mixamo's do."""

# role, parent role, world joint, where the bone points, roll about itself
Spec = tuple[str, str | None, Vec3, Vec3, float]

# Our own rig: auto-rigger output, so every bone carries an arbitrary roll,
# the arms sit below horizontal and lean forward, and `hips` points out of a
# hip socket rather than up the spine, which is the committed rig's own
# defect. Nothing here is planar: three directions in one plane would make
# the aim table's rows cancel out, and then no test could see a wrong row.
OURS: tuple[Spec, ...] = (
    ("hips", None, (0.0, 0.0, 1.00), (-0.98, -0.145, -0.136), 0.0),
    ("spine_lower", "hips", (0.0, 0.0, 1.10), (0.0, -0.08, 1.0), 5.0),
    ("spine_middle", "spine_lower", (0.0, 0.0, 1.25), (0.0, -0.05, 1.0), 5.0),
    ("neck", "spine_middle", (0.0, 0.0, 1.45), (0.0, -0.3, 1.0), 3.0),
    ("head", "neck", (0.0, -0.03, 1.55), (0.0, -0.2, 1.0), 3.0),
    ("left_arm", "spine_middle", (0.18, 0.0, 1.40), (1.0, -0.15, -1.0), -15.0),
    ("left_forearm", "left_arm", (0.40, -0.03, 1.18), (1.0, -0.15, -1.0), -15.0),
    ("left_hand", "left_forearm", (0.60, -0.06, 0.98), (1.0, -0.15, -1.0), -15.0),
    ("right_arm", "spine_middle", (-0.18, 0.0, 1.40), (-1.0, -0.15, -1.0), 15.0),
    ("right_forearm", "right_arm", (-0.40, -0.03, 1.18), (-1.0, -0.15, -1.0), 15.0),
    ("right_hand", "right_forearm", (-0.60, -0.06, 0.98), (-1.0, -0.15, -1.0), 15.0),
    ("left_upper_leg", "hips", (0.09, 0.0, 0.95), LEG, 0.0),
    ("left_leg", "left_upper_leg", (0.09, 0.02, 0.55), LEG, 0.0),
    ("right_upper_leg", "hips", (-0.09, 0.0, 0.95), LEG, 0.0),
    ("right_leg", "right_upper_leg", (-0.09, 0.02, 0.55), LEG, 0.0),
)

# The rest twist our rig and a bought one disagree by. Legs and feet sit 171
# to 175 degrees of pure roll apart between the committed rig and Mixamo's,
# and a correct fit preserves that difference rather than reading 5 degrees.
THIGH_ROLL_DEGREES = 174.0

# A bought rig: T-posed rather than A-posed, taller, longer limbs, rolled
# differently on every part. One aim table has to describe both rest
# conventions, which is what makes the table load bearing: aiming two rigs
# whose arms point elsewhere costs each of them a different swing.
SOURCE: tuple[Spec, ...] = (
    ("hips", None, (0.0, 0.0, 1.05), (0.0, -0.02, 1.0), 0.0),
    ("spine_lower", "hips", (0.0, 0.0, 1.16), (0.0, 0.04, 1.0), 10.0),
    ("spine_middle", "spine_lower", (0.0, 0.0, 1.33), (0.0, 0.02, 1.0), 10.0),
    ("neck", "spine_middle", (0.0, 0.0, 1.56), (0.0, -0.1, 1.0), -6.0),
    ("head", "neck", (0.0, -0.01, 1.67), (0.0, -0.05, 1.0), -6.0),
    ("left_arm", "spine_middle", (0.20, 0.0, 1.50), (1.0, -0.35, 0.0), 30.0),
    ("left_forearm", "left_arm", (0.48, -0.1, 1.50), (1.0, -0.35, 0.0), 30.0),
    ("left_hand", "left_forearm", (0.74, -0.2, 1.50), (1.0, -0.35, 0.0), 30.0),
    ("right_arm", "spine_middle", (-0.20, 0.0, 1.50), (-1.0, -0.35, 0.0), -30.0),
    ("right_forearm", "right_arm", (-0.48, -0.1, 1.50), (-1.0, -0.35, 0.0), -30.0),
    ("right_hand", "right_forearm", (-0.74, -0.2, 1.50), (-1.0, -0.35, 0.0), -30.0),
    (
        "left_upper_leg",
        "hips",
        (0.11, 0.0, 1.00),
        (0.0, -0.03, -1.0),
        THIGH_ROLL_DEGREES,
    ),
    (
        "left_leg",
        "left_upper_leg",
        (0.11, -0.01, 0.52),
        (0.0, -0.03, -1.0),
        THIGH_ROLL_DEGREES,
    ),
    (
        "right_upper_leg",
        "hips",
        (-0.11, 0.0, 1.00),
        (0.0, -0.03, -1.0),
        -THIGH_ROLL_DEGREES,
    ),
    (
        "right_leg",
        "right_upper_leg",
        (-0.11, -0.01, 0.52),
        (0.0, -0.03, -1.0),
        -THIGH_ROLL_DEGREES,
    ),
)

CHAIN: dict[str, str] = {
    role: parent for role, parent, *_ in OURS if parent is not None
}

AIM: dict[str, Vec3] = {
    "hips": UP,
    "spine_lower": UP,
    "spine_middle": UP,
    "neck": UP,
    "head": UP,
    # Half way between the two rest conventions, the way the real table sits
    # 45 degrees below horizontal so it can describe an A-pose and a T-pose.
    "left_arm": OUT_LEFT,
    "left_forearm": OUT_LEFT,
    "left_hand": OUT_LEFT,
    "right_arm": OUT_RIGHT,
    "right_forearm": OUT_RIGHT,
    "right_hand": OUT_RIGHT,
    "left_upper_leg": LEG,
    "left_leg": LEG,
    "right_upper_leg": LEG,
    "right_leg": LEG,
}

# Our own bone names, so the target is addressed the way a rig is.
BONE_OF_ROLE = {
    "hips": "Hips",
    "spine_lower": "Spine",
    "spine_middle": "Spine1",
    "neck": "Neck",
    "head": "Head",
    "left_arm": "LeftArm",
    "left_forearm": "LeftForeArm",
    "left_hand": "LeftHand",
    "right_arm": "RightArm",
    "right_forearm": "RightForeArm",
    "right_hand": "RightHand",
    "left_upper_leg": "LeftUpLeg",
    "left_leg": "LeftLeg",
    "right_upper_leg": "RightUpLeg",
    "right_leg": "RightLeg",
}


def frame(joint: Vec3, aim: Vec3, roll: float) -> Mat4:
    """A bone at `joint` pointing along `aim`, rolled about its own axis."""
    rotation = quat_multiply(
        aim_rotation(CHILD_AXIS, aim), quat_from_axis(CHILD_AXIS, roll)
    )
    basis = _rotation_matrix(rotation)
    return (
        (*basis[0][:3], joint[0]),
        (*basis[1][:3], joint[1]),
        (*basis[2][:3], joint[2]),
        (0.0, 0.0, 0.0, 1.0),
    )


def _rotation_matrix(q: Quat) -> Mat4:
    """A rotation as a matrix, built here so the module under test is not the
    only source of its own fixtures."""
    w, x, y, z = quat_normalized(q)
    return (
        (1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w), 0.0),
        (2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w), 0.0),
        (2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y), 0.0),
        (0.0, 0.0, 0.0, 1.0),
    )


UNIT = 0.01
"""Meters per armature unit, which both importers hand us as the object's
scale. A rig at 0.01 carries joint translations 100 times larger than the
meters they mean, and that is the whole of fact 7: applying the object
transform rescales the rest and leaves every location key byte identical, so
2.316 m of travel becomes 231.599 m.
"""


def rest_local(spec: tuple[Spec, ...]) -> dict[str, Mat4]:
    """Rest frames in ARMATURE units, the way `bone.matrix_local` holds them.

    Orthonormal, with the joint divided by the object's scale, so composing
    the object matrix gives meters back.
    """
    return {
        role: frame(scaled(joint, 1.0 / UNIT), aim, roll)
        for role, _, joint, aim, roll in spec
    }


def in_world(local: dict[str, Mat4], obj: Mat4) -> dict[str, Mat4]:
    """`matrix_world @ bone.matrix_local`, which is how a rest pose is read."""
    return {role: mat_multiply(obj, matrix) for role, matrix in local.items()}


def our_bones(local: dict[str, Mat4]) -> tuple[Bone, ...]:
    """Our rig, in armature space, as the transfer takes it.

    Carries `head_end`, which fills no role on the committed rig: the code
    this replaces raised on exactly that shape.
    """
    bones = [
        Bone(
            name=BONE_OF_ROLE[role],
            parent=None if parent is None else BONE_OF_ROLE[parent],
            role=role,
            rest=local[role],
        )
        for role, parent, *_ in OURS
    ]
    tip = frame(scaled((0.0, -0.05, 1.75), 1.0 / UNIT), UP, 0.0)
    bones.append(Bone(name="head_end", parent="Head", role=None, rest=tip))
    return tuple(bones)


def posed_world(
    spec: tuple[Spec, ...],
    local: dict[str, Mat4],
    obj: Mat4,
    turns: dict[str, Quat],
    travel: Vec3 = (0.0, 0.0, 0.0),
) -> dict[str, Mat4]:
    """A rig's world matrices with `turns` keyed on it, by forward kinematics.

    The source clip, built the way Blender evaluates one: each bone's own
    local rotation, under its parent's result. `travel` moves the root in
    world space, so the root's own location key is never trivially zero.
    """
    rest_basis = {
        role: (
            local[role]
            if parent is None
            else mat_multiply(mat_inverted(local[parent], parent), local[role])
        )
        for role, parent, *_ in spec
    }
    posed: dict[str, Mat4] = {}
    for role, parent, *_ in spec:
        above = IDENTITY if parent is None else posed[parent]
        turn = _rotation_matrix(turns.get(role, NO_ROTATION))
        posed[role] = mat_multiply(above, mat_multiply(rest_basis[role], turn))
    moved = _translation(travel)
    return {
        role: mat_multiply(moved, mat_multiply(obj, matrix))
        for role, matrix in posed.items()
    }


def output_world(
    bones: tuple[Bone, ...], poses: dict[str, LocalPose]
) -> dict[str, Mat4]:
    """Our rig's world matrices after the transfer, by forward kinematics.

    An independent path back: the transfer hands out local poses, and this
    composes them the way Blender does, so nothing below reads a world matrix
    the transfer computed.
    """
    by_name = {bone.name: bone for bone in bones}
    posed: dict[str, Mat4] = {}
    for bone in bones:
        parent = None if bone.parent is None else by_name[bone.parent]
        rest_basis = (
            bone.rest
            if parent is None
            else mat_multiply(mat_inverted(parent.rest, "parent"), bone.rest)
        )
        above = IDENTITY if parent is None else posed[parent.name]
        basis = IDENTITY
        if (pose := poses.get(bone.name)) is not None:
            basis = _rotation_matrix(pose.rotation)
            if pose.location is not None:
                basis = mat_multiply(_translation(pose.location), basis)
        posed[bone.name] = mat_multiply(above, mat_multiply(rest_basis, basis))
    return {name: mat_multiply(OUR_OBJECT, matrix) for name, matrix in posed.items()}


def apart(a: Vec3, b: Vec3) -> float:
    """How far two points sit from each other, in meters."""
    return length((a[0] - b[0], a[1] - b[1], a[2] - b[2]))


def degrees_between(a: Vec3, b: Vec3) -> float:
    """The angle between two directions, in degrees.

    From the cross product rather than from `acos` of the dot product:
    `acos` near 1 turns machine epsilon into 1e-6 degrees, which would put a
    floor under every number below and hide a real thousandth of a degree.
    """
    left, right = normalized(a), normalized(b)
    return math.degrees(math.atan2(length(cross(left, right)), dot(left, right)))


A_CLIP: dict[str, Quat] = {
    # One frame of a source clip: something moving on every part of the body.
    "hips": quat_from_axis((0.0, 0.0, 1.0), 12.0),
    "spine_lower": quat_from_axis((1.0, 0.0, 0.0), 7.0),
    "neck": quat_from_axis((1.0, 0.0, 0.0), 35.0),
    "left_arm": quat_from_axis((0.0, 1.0, 0.0), 20.0),
    "left_forearm": quat_from_axis((0.0, 0.0, 1.0), 55.0),
    "right_forearm": quat_from_axis((0.0, 0.0, 1.0), -40.0),
    "left_upper_leg": quat_from_axis((1.0, 0.0, 0.0), 25.0),
    "left_leg": quat_from_axis((1.0, 0.0, 0.0), -30.0),
}

TRAVEL: Vec3 = (2.31, 0.0, 0.04)
"""What a bought strafe travels: 2.31 m sideways and a 4 cm bob."""


class Fitted:
    """One whole fit, so a test states only what it varies.

    `source` defaults to the bought rig. Handed `OURS` it is the refit case,
    our own clip on our own rig, where every offset must be identity.
    """

    def __init__(
        self,
        *,
        aim: dict[str, Vec3] | None = None,
        source: tuple[Spec, ...] = SOURCE,
        source_object: Mat4 = SOURCE_OBJECT,
        object_matrix: Mat4 = OUR_OBJECT,
        scale: float = 1.0,
    ) -> None:
        table = AIM if aim is None else aim
        source_object = mat_multiply(_uniform(scale), source_object)
        self.ours_local = rest_local(OURS)
        self.source_local = rest_local(source)
        self.ours_rest = in_world(self.ours_local, OUR_OBJECT)
        self.source_rest = in_world(self.source_local, source_object)
        self.bones = our_bones(self.ours_local)
        self.reference_ours = reference_pose(self.ours_rest, CHAIN, table)
        self.reference_source = reference_pose(self.source_rest, CHAIN, table)
        self.offset = offsets(self.reference_ours, self.reference_source)
        self.world_src = posed_world(
            source, self.source_local, source_object, A_CLIP, TRAVEL
        )
        self.poses = transfer(self.bones, object_matrix, self.world_src, self.offset)
        self.world_out = output_world(self.bones, self.poses)

    def swing_error(self, role: str) -> float:
        """How far our bone points from where the source's bone points.

        Absolute, against the source clip, and a correct fit reads 0: aiming
        both rigs at one table makes every offset a pure twist about the
        bone's own axis, and a twist cannot move where the bone points.
        """
        return degrees_between(
            mat_direction(self.world_out[BONE_OF_ROLE[role]], CHILD_AXIS),
            mat_direction(self.world_src[role], CHILD_AXIS),
        )

    def worst_swing(self) -> tuple[str, float]:
        worst = max(self.offset, key=self.swing_error)
        return worst, self.swing_error(worst)

    def worst_swing_with(self, offset: dict[str, Quat]) -> float:
        """The same measurement, on a fit driven by some other offsets.

        Every mutation below that only re-rolls a bone reads 0 here, and
        that is the whole reason the verifier needs a twist rule beside its
        swing rule.
        """
        world = output_world(
            self.bones, transfer(self.bones, OUR_OBJECT, self.world_src, offset)
        )
        return max(
            degrees_between(
                mat_direction(world[BONE_OF_ROLE[role]], CHILD_AXIS),
                mat_direction(self.world_src[role], CHILD_AXIS),
            )
            for role in offset
        )

    def bone_lengths(self) -> dict[str, float]:
        """Our own joint to joint distances, after the fit, in meters."""
        return {
            role: segment_length(
                {r: self.world_out[BONE_OF_ROLE[r]] for r in (role, parent)},
                parent,
                role,
            )
            for role, parent, *_ in OURS
            if parent is not None
        }

    def offset_degrees(self, role: str, against: dict[str, Quat]) -> float:
        """How far this fit's offset for one role sits from another's."""
        return quat_degrees(
            quat_multiply(quat_inverted(against[role]), self.offset[role])
        )


def _translation(offset: Vec3) -> Mat4:
    return (
        (1.0, 0.0, 0.0, offset[0]),
        (0.0, 1.0, 0.0, offset[1]),
        (0.0, 0.0, 1.0, offset[2]),
        (0.0, 0.0, 0.0, 1.0),
    )


def _uniform(factor: float) -> Mat4:
    return (
        (factor, 0.0, 0.0, 0.0),
        (0.0, factor, 0.0, 0.0),
        (0.0, 0.0, factor, 0.0),
        (0.0, 0.0, 0.0, 1.0),
    )


# --- known answers, as numbers a human computed once ----------------------


def wrong_split(rotation: Quat, axis: Vec3) -> tuple[Quat, Quat]:
    """The split with `Quaternion.axis` in place of the vector part.

    Kept here rather than described: `axis` is normalized, so it drops the
    `sin(angle / 2)` factor, and the fault is worst near identity where a
    pure swing hides it. The two numbers below are what it produces.
    """
    unit = normalized(axis)
    axis_only = normalized((rotation[1], rotation[2], rotation[3]))
    projected = scaled(unit, dot(axis_only, unit))
    twist = quat_normalized((rotation[0], *projected))
    return quat_multiply(rotation, quat_inverted(twist)), twist


def test_a_ten_degree_twist_about_the_bone_axis_is_all_twist() -> None:
    swing, twist = swing_twist(quat_from_axis(CHILD_AXIS, 10.0), CHILD_AXIS, "bone")

    assert round(quat_degrees(twist), 2) == 10.00
    assert round(quat_degrees(swing), 2) == 0.00


def test_the_normalized_axis_form_reads_90_22_on_that_same_twist() -> None:
    swing, twist = wrong_split(quat_from_axis(CHILD_AXIS, 10.0), CHILD_AXIS)

    assert round(quat_degrees(twist), 2) == 90.22
    assert round(quat_degrees(swing), 2) == 80.22


def test_a_swing_and_a_twist_together_come_back_as_themselves() -> None:
    """`rotation == swing @ twist`, and the order is the whole content of that
    line: a pure twist and a pure swing both split the same way whichever way
    round it is composed, so only a rotation carrying both can tell them
    apart. Built from 30 degrees about +X and 40 about +Y, and both come back
    on the axis they went in on."""
    swing_in = quat_from_axis((1.0, 0.0, 0.0), 30.0)
    twist_in = quat_from_axis(CHILD_AXIS, 40.0)

    swing, twist = swing_twist(quat_multiply(swing_in, twist_in), CHILD_AXIS, "bone")

    assert round(quat_degrees(swing), 2) == 30.00
    assert round(quat_degrees(twist), 2) == 40.00
    # And on their own axes: the wrong composition order still reads 30
    # degrees of swing, about an axis that has picked up a Z component.
    assert swing[3] == pytest.approx(0.0, abs=1e-12), "the swing is about +X"
    assert twist[1] == pytest.approx(0.0, abs=1e-12), "the twist is about +Y"


def test_a_thirty_degree_swing_across_the_bone_axis_is_all_swing() -> None:
    swing, twist = swing_twist(quat_from_axis((1.0, 0.0, 0.0), 30.0), CHILD_AXIS, "b")

    assert round(quat_degrees(swing), 2) == 30.00
    assert round(quat_degrees(twist), 2) == 0.00


def test_half_a_turn_across_the_bone_axis_has_no_twist_to_report() -> None:
    with pytest.raises(TransferError) as raised:
        swing_twist(quat_from_axis((1.0, 0.0, 0.0), 180.0), CHILD_AXIS, "LeftUpLeg")

    assert (raised.value.code, raised.value.subject) == (
        "swing_singular",
        "LeftUpLeg",
    )


def test_two_rigs_a_roll_apart_offset_by_exactly_that_roll() -> None:
    """The design's own known answer, on the case where nothing else can
    contribute: two rigs whose legs point the same way and are rolled 174
    degrees apart. Anything but 174 has thrown that difference away."""
    hips = frame((0.0, 0.0, 100.0), UP, 0.0)
    thigh = (9.0, 0.0, 95.0)
    ours = {"hips": hips, "left_upper_leg": frame(thigh, DOWN, 0.0)}
    theirs = {
        "hips": hips,
        "left_upper_leg": frame(thigh, DOWN, THIGH_ROLL_DEGREES),
    }
    chain = {"left_upper_leg": "hips"}
    aim = {"hips": UP, "left_upper_leg": DOWN}

    offset = offsets(
        reference_pose(in_world(ours, OUR_OBJECT), chain, aim),
        reference_pose(in_world(theirs, OUR_OBJECT), chain, aim),
    )

    assert round(quat_degrees(offset["left_upper_leg"]), 2) == 174.00
    assert round(quat_degrees(offset["hips"]), 2) == 0.00


def test_every_offset_is_a_pure_twist_and_the_thigh_is_never_identity() -> None:
    """Aiming both rigs at one table is what makes every offset a rotation
    about the bone's own axis and nothing else. That is the property the
    whole method rests on: a twist cannot move where a bone points, so the
    swing comes out of the source untouched and the table decides only the
    roll.

    On the whole rig the thigh's number moves off 174, because our own `hips`
    points out of a hip socket and every child's reference frame carries that
    correction. What cannot move is that the offset is pure roll.
    """
    fit = Fitted()

    for role, offset in fit.offset.items():
        swing, twist = swing_twist(offset, CHILD_AXIS, role)
        assert quat_degrees(swing) == pytest.approx(0.0, abs=1e-5), role
        assert quat_degrees(twist) == pytest.approx(quat_degrees(offset), abs=1e-5), (
            role
        )
    swing, twist = swing_twist(fit.offset["left_upper_leg"], CHILD_AXIS, "thigh")
    assert round(quat_degrees(twist), 3) == 142.486
    assert quat_degrees(swing) == pytest.approx(0.0, abs=1e-5)


def test_our_own_clip_on_our_own_rig_offsets_by_nothing() -> None:
    """The refit: the rename invalidates the two committed Meshy clips, and
    they are fitted again through this. Same body, so every offset is
    identity, and a fit that reports otherwise has invented a correction."""
    refit = Fitted(source=OURS, source_object=OUR_OBJECT)

    for role, offset in refit.offset.items():
        # A rotation read out of a matrix and back carries about 1e-6
        # degrees, which is the floor of every number in this file.
        assert quat_degrees(offset) == pytest.approx(0.0, abs=1e-4), role
    assert refit.worst_swing()[1] == pytest.approx(0.0, abs=1e-9)


# --- the absolute oracle, against the source clip -------------------------


def test_every_bone_points_exactly_where_the_source_bone_points() -> None:
    fit = Fitted()

    for role in fit.offset:
        assert fit.swing_error(role) == pytest.approx(0.0, abs=1e-9), role


def test_the_worst_bone_of_a_correct_fit_is_still_zero() -> None:
    role, error = Fitted().worst_swing()

    assert error < 1e-9, f"{role} is {error} degrees out"


def test_the_root_lands_exactly_on_the_sources_root() -> None:
    fit = Fitted()

    assert mat_translation(fit.world_out["Hips"]) == pytest.approx(
        mat_translation(fit.world_src["hips"]), abs=1e-12
    )


def test_no_other_joint_lands_on_the_sources_joint() -> None:
    """Our limbs are shorter, and they stay shorter: only the root is placed,
    every other bone is rotated. A hand that lands on the source's hand has
    inherited the source's forearm."""
    fit = Fitted()

    assert apart(
        mat_translation(fit.world_out["LeftHand"]),
        mat_translation(fit.world_src["left_hand"]),
    ) == pytest.approx(0.1972, abs=0.0001)


# --- the mutations, each moving one of those numbers ----------------------


def test_the_object_transform_composed_and_not_applied() -> None:
    """Requirement 1. Left out, which is what applying it leaves behind, the
    root lands 2.8 m from where the source's root is: the same class of fault
    as fact 7's 2.316 m reading 231.599 m."""
    applied = Fitted(object_matrix=IDENTITY)

    off = apart(
        mat_translation(applied.world_out["Hips"]),
        mat_translation(applied.world_src["hips"]),
    )
    assert off == pytest.approx(2.8072, abs=0.0001)


def test_the_offset_applied_on_the_left_moves_every_limb() -> None:
    fit = Fitted()

    wrong = transfer(
        fit.bones,
        OUR_OBJECT,
        {
            role: mat_multiply(_rotation_matrix(fit.offset[role]), matrix)
            for role, matrix in fit.world_src.items()
        },
        dict.fromkeys(fit.offset, NO_ROTATION),
    )
    world = output_world(fit.bones, wrong)
    worst = max(
        degrees_between(
            mat_direction(world[BONE_OF_ROLE[role]], CHILD_AXIS),
            mat_direction(fit.world_src[role], CHILD_AXIS),
        )
        for role in fit.offset
    )
    assert worst == pytest.approx(153.584, abs=0.001)


def test_the_aim_table_fed_where_a_reference_pose_belongs() -> None:
    """`offsets` takes two reference poses and never the table. Handed the
    table's own directions it returns a re-roll of every bone, which no swing
    measurement can see: the offsets stay pure twists, so the clip still
    points its bones correctly and rolls all of them wrong. That is why the
    verifier has a twist rule beside its swing rule."""
    fit = Fitted()
    as_table = {role: frame((0.0, 0.0, 0.0), aim, 0.0) for role, aim in AIM.items()}

    wrong = offsets(fit.reference_ours, as_table)

    assert round(quat_degrees(wrong["left_upper_leg"]), 3) == 6.484
    worst = max(
        quat_degrees(quat_multiply(quat_inverted(wrong[role]), fit.offset[role]))
        for role in fit.offset
    )
    assert worst == pytest.approx(160.970, abs=0.001)
    # And the half that matters: every bone still points exactly where the
    # source's bone points, so a swing rule reads this fit as perfect.
    assert fit.worst_swing_with(wrong) == pytest.approx(0.0, abs=1e-9)


def test_a_mirror_row_with_the_wrong_sign_re_rolls_the_arm_it_names() -> None:
    fit = Fitted()

    broken = Fitted(aim=dict(AIM) | {"right_arm": OUT_LEFT})

    assert broken.offset_degrees("right_arm", fit.offset) == pytest.approx(
        64.923, abs=0.001
    )
    # Only that side. The table's mirror rows are held to an exact reflection
    # at load time, and this is what the wrong sign would have cost.
    assert broken.offset_degrees("left_arm", fit.offset) == pytest.approx(0.0, abs=1e-4)
    # And the swing metric sees none of it: the re-roll is a pure twist.
    assert fit.worst_swing_with(broken.offset) == pytest.approx(0.0, abs=1e-9)


def test_swapping_left_and_right_moves_both_arms() -> None:
    fit = Fitted()
    swapped = dict(fit.world_src)
    for left in ("left_arm", "left_forearm", "left_hand"):
        right = left.replace("left", "right")
        swapped[left], swapped[right] = fit.world_src[right], fit.world_src[left]

    world = output_world(
        fit.bones, transfer(fit.bones, OUR_OBJECT, swapped, fit.offset)
    )

    off = degrees_between(
        mat_direction(world["LeftForeArm"], CHILD_AXIS),
        mat_direction(fit.world_src["left_forearm"], CHILD_AXIS),
    )
    assert off == pytest.approx(111.304, abs=0.001)


def test_a_role_the_source_leaves_out_is_stepped_over() -> None:
    """The chain is keyed by role for this reason: a source with three spine
    bones drives a target with four, and everything below the missing one
    still lands where the source has it."""
    fit = Fitted()
    without = {r: m for r, m in fit.world_src.items() if r != "spine_middle"}
    chain = {
        role: ("spine_lower" if parent == "spine_middle" else parent)
        for role, parent in CHAIN.items()
        if role != "spine_middle"
    }
    offset = offsets(
        reference_pose(_without(fit.ours_rest), chain, AIM),
        reference_pose(_without(fit.source_rest), chain, AIM),
    )

    poses = transfer(fit.bones, OUR_OBJECT, without, offset)
    world = output_world(fit.bones, poses)

    assert "Spine1" not in poses
    for role in ("neck", "head", "left_hand", "right_hand"):
        off = degrees_between(
            mat_direction(world[BONE_OF_ROLE[role]], CHILD_AXIS),
            mat_direction(fit.world_src[role], CHILD_AXIS),
        )
        assert off == pytest.approx(0.0, abs=1e-9), role


def _without(rest: dict[str, Mat4]) -> dict[str, Mat4]:
    return {role: matrix for role, matrix in rest.items() if role != "spine_middle"}


# --- rotation-only keys ---------------------------------------------------


def test_only_the_root_carries_a_location() -> None:
    fit = Fitted()

    assert fit.poses["Hips"].location is not None
    for name, pose in fit.poses.items():
        if name != "Hips":
            assert pose.location is None, name


def test_a_bone_with_no_role_is_skipped_and_not_raised_on() -> None:
    fit = Fitted()

    assert "head_end" not in fit.poses
    assert any(bone.name == "head_end" for bone in fit.bones)


def test_a_source_scaled_by_one_and_a_half_gives_the_same_bone_lengths() -> None:
    plain, larger = Fitted(), Fitted(scale=1.5)

    assert larger.bone_lengths() == pytest.approx(plain.bone_lengths(), rel=1e-9)
    # And they are our own rest lengths, not a share of the source's.
    ours = in_world(rest_local(OURS), OUR_OBJECT)
    for role, parent, *_ in OURS:
        if parent is not None:
            assert plain.bone_lengths()[role] == pytest.approx(
                segment_length(ours, parent, role), rel=1e-9
            )


def test_a_femur_is_measured_from_the_two_joints_it_runs_between() -> None:
    ours = in_world(rest_local(OURS), OUR_OBJECT)
    theirs = in_world(rest_local(SOURCE), SOURCE_OBJECT)

    assert segment_length(ours, "left_upper_leg", "left_leg") == pytest.approx(
        0.40050, abs=1e-5
    )
    assert segment_length(theirs, "left_upper_leg", "left_leg") == pytest.approx(
        0.48010, abs=1e-5
    )


# --- refusals -------------------------------------------------------------


def test_a_role_with_no_aim_row_is_refused_rather_than_guessed() -> None:
    rest = in_world(rest_local(OURS), OUR_OBJECT)

    with pytest.raises(TransferError) as raised:
        reference_pose(rest, CHAIN, {k: v for k, v in AIM.items() if k != "head"})

    assert (raised.value.code, raised.value.subject) == ("aim_row_missing", "head")


def test_a_chain_that_never_reaches_a_top_is_refused() -> None:
    with pytest.raises(TransferError) as raised:
        reference_pose({"head": frame((0.0, 0.0, 1.0), UP, 0.0)}, {"head": "neck"}, AIM)

    assert (raised.value.code, raised.value.subject) == ("role_unmapped", "head")


def test_two_reference_poses_that_disagree_about_a_role_are_refused() -> None:
    fit = Fitted()

    with pytest.raises(TransferError) as raised:
        offsets(
            fit.reference_ours,
            {k: v for k, v in fit.reference_source.items() if k != "head"},
        )

    assert (raised.value.code, raised.value.subject) == ("role_unmapped", "head")


def test_a_frame_with_no_offset_for_it_is_refused() -> None:
    fit = Fitted()

    with pytest.raises(TransferError) as raised:
        transfer(
            fit.bones,
            OUR_OBJECT,
            fit.world_src,
            {k: v for k, v in fit.offset.items() if k != "head"},
        )

    assert (raised.value.code, raised.value.subject) == ("role_unmapped", "head")


def test_a_bone_whose_parent_is_not_in_the_rig_is_refused() -> None:
    fit = Fitted()
    orphan = Bone(name="Tail", parent="Pelvis", role=None, rest=IDENTITY)

    with pytest.raises(TransferError) as raised:
        transfer((*fit.bones, orphan), OUR_OBJECT, fit.world_src, fit.offset)

    assert (raised.value.code, raised.value.subject) == ("bone_missing", "Tail")


def test_a_femur_needs_both_of_its_joints() -> None:
    rest = in_world(rest_local(OURS), OUR_OBJECT)

    with pytest.raises(TransferError) as raised:
        segment_length(rest, "left_upper_leg", "left_foot")

    assert (raised.value.code, raised.value.subject) == ("role_unmapped", "left_foot")


def test_a_rig_scaled_a_thousandth_still_inverts() -> None:
    """A determinant of 1e-9 is a small rig, not a flat one. The refusal is
    against the volume the three columns could enclose, so scale alone never
    trips it."""
    tiny = mat_multiply(_uniform(1e-3), frame((1.0, 2.0, 3.0), OUT_LEFT, 20.0))

    product = mat_multiply(tiny, mat_inverted(tiny, "bone"))

    for row in range(4):
        assert product[row] == pytest.approx(IDENTITY[row], abs=1e-9)


def test_a_matrix_with_no_volume_holds_no_transform() -> None:
    flat: Mat4 = ((0.0, 0.0, 0.0, 0.0), IDENTITY[1], IDENTITY[2], IDENTITY[3])

    with pytest.raises(TransferError) as raised:
        mat_inverted(flat, "Hips")

    assert (raised.value.code, raised.value.subject) == ("degenerate_basis", "Hips")


def test_a_left_handed_frame_holds_no_rotation() -> None:
    mirrored: Mat4 = ((-1.0, 0.0, 0.0, 0.0), IDENTITY[1], IDENTITY[2], IDENTITY[3])

    with pytest.raises(TransferError) as raised:
        mat_rotation(mirrored, "Hips")

    assert (raised.value.code, raised.value.subject) == ("degenerate_basis", "Hips")


def test_a_direction_of_no_length_is_refused() -> None:
    with pytest.raises(TransferError) as raised:
        normalized((0.0, 0.0, 0.0))

    assert raised.value.code == "no_direction"


def test_a_quaternion_of_no_length_is_refused() -> None:
    with pytest.raises(TransferError) as raised:
        quat_normalized((0.0, 0.0, 0.0, 0.0))

    assert raised.value.code == "no_direction"


# --- the small maths, each with its own answer ----------------------------


def test_the_aim_of_a_direction_onto_itself_is_no_rotation() -> None:
    assert aim_rotation(UP, (0.0, 0.0, 2.0)) == NO_ROTATION


def test_the_aim_of_a_direction_onto_its_opposite_is_half_a_turn() -> None:
    half = aim_rotation(UP, DOWN)

    assert round(quat_degrees(half), 2) == 180.00
    # About an axis square to the direction, so it is a swing and not a roll.
    assert dot((half[1], half[2], half[3]), UP) == pytest.approx(0.0, abs=1e-12)


@pytest.mark.parametrize(
    ("aim", "expected"),
    [(UP, (0.0, 0.0, 1.0)), (OUT_LEFT, (0.7071067812, 0.0, -0.7071067812))],
)
def test_a_built_bone_points_where_it_was_told_to(aim: Vec3, expected: Vec3) -> None:
    pointing = mat_direction(frame((1.0, 2.0, 3.0), aim, 45.0), CHILD_AXIS)

    assert pointing == pytest.approx(expected, abs=1e-9)


def test_a_bones_own_position_is_its_translation() -> None:
    assert mat_translation(frame((1.0, 2.0, 3.0), UP, 0.0)) == (1.0, 2.0, 3.0)


@pytest.mark.parametrize("degrees", [0.0, 30.0, 90.0, 179.0])
def test_a_rotation_survives_a_trip_through_a_matrix(degrees: float) -> None:
    """Every branch of the extraction, chosen by which component is largest."""
    for axis in ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0)):
        turn = quat_from_axis(axis, degrees)
        back = mat_rotation(_rotation_matrix(turn), "bone")
        # An `acos` of a residual near identity has square root error, so
        # this is machine epsilon rather than a real drift.
        assert quat_degrees(quat_multiply(quat_inverted(back), turn)) == (
            pytest.approx(0.0, abs=1e-5)
        )


def test_a_scaled_matrix_holds_the_same_rotation_as_a_plain_one() -> None:
    turn = quat_from_axis((1.0, 2.0, 3.0), 40.0)
    scaled_up = mat_multiply(_uniform(7.0), _rotation_matrix(turn))

    assert mat_rotation(scaled_up, "bone") == pytest.approx(turn, abs=1e-9)


def test_an_inverse_undoes_its_matrix() -> None:
    matrix = frame((1.0, 2.0, 3.0), OUT_LEFT, 20.0)

    product = mat_multiply(matrix, mat_inverted(matrix, "bone"))

    for row in range(4):
        assert product[row] == pytest.approx(IDENTITY[row], abs=1e-12)


def test_the_small_vector_maths() -> None:
    assert dot((1.0, 2.0, 3.0), (4.0, 5.0, 6.0)) == 32.0
    assert cross((1.0, 0.0, 0.0), (0.0, 1.0, 0.0)) == (0.0, 0.0, 1.0)
    assert length((3.0, 4.0, 0.0)) == 5.0
    assert scaled((1.0, 2.0, 3.0), 2.0) == (2.0, 4.0, 6.0)
    assert normalized((0.0, 5.0, 0.0)) == (0.0, 1.0, 0.0)


def test_a_quaternion_and_its_negation_are_the_same_turn() -> None:
    turn = quat_from_axis(UP, 90.0)
    flipped: Quat = (-turn[0], -turn[1], -turn[2], -turn[3])

    assert quat_degrees(flipped) == pytest.approx(quat_degrees(turn))
    assert quat_multiply(turn, quat_inverted(turn)) == pytest.approx(
        NO_ROTATION, abs=1e-12
    )
