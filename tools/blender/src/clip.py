"""What the retarget reports about the clip it just wrote.

Three jobs, all free of `bpy`.

- **The counts.** Requirement 9, per bone: channels that are not straight
  lines between the frames the transfer wrote, and keys outside the frames the
  source had. Both are the transfer's own invariants, so both are structurally
  true when the code is right and worth nothing unless something can see them
  break.
- **The frame grid.** Requirement 3: the scene runs at the clip's own
  `source_fps`, so every key of the source lands on a whole frame and the
  render range is the action's own. This is the only place an off-grid import
  is visible at all. The retarget samples whole frames, so what it writes out
  sits on the grid whatever it read: a 30 fps clip read in a 24 fps scene
  spans 0.8 to 16.8, and rounding that to 1 to 17 drops four frames and
  exports a file nothing downstream can tell from a correct one.
- **The source motion sidecar.** `clip.swing`, `clip.twist` and
  `clip.posture` measure the delivered GLB against the vendor file, and the
  vendor file is an FBX that the Rust `gltf` reader cannot open. So this run
  writes the source's own world orientations and joints beside the report, and
  Rust reads the GLB and the sidecar together.

Reading an F-curve or a pose needs `bpy`. Counting and recording do not, so
they are here and `retarget_animation.py` only hands over what it read. The
Rust side owns the rules themselves, in
`crates/xtask-art/src/check/clip.rs`, which is what
`cargo art check --list-rules` prints.

Free of `bpy`, so it is unit tested with no Blender.
"""

import pathlib
from collections.abc import Iterable, Mapping, Sequence

from findings import Comparison, Finding, Rule
from framing import Frozen, Vec3, bone_from_data_path
from pydantic import model_validator
from transfer import Mat4, Quat, mat_rotation, mat_translation

LINEAR = "LINEAR"
"""The only interpolation the retarget writes. `keyframe_insert` writes
Bezier, which rounds off every joint's path between two sampled frames."""

CONSTANT = "CONSTANT"
"""The only extrapolation it writes: outside the clip the pose is held."""

CHANNELS = "the F-curves of the output action, before export"

GRID = "each key of the clip, against the frame grid its declared source_fps sets"

RANGE = "the frames the retarget samples, against the source's own key times"

GROUND = (
    "every ground joint at every frame of the clip, in Blender Z-up world "
    "space, after the floor snap"
)

SIZED = (
    "the root joint's horizontal travel, first frame to last, against the "
    "source's own travel sized by the femur ratio, in Blender Z-up world space"
)

SEGMENT = (
    "the two stride_segment joints of each rig at rest, in Blender Z-up world space"
)

INTERPOLATION = Rule(
    id="clip.interpolation",
    comparison=Comparison.EQ,
    unit="channels",
    measured_on=CHANNELS,
)

REFERENCE_POSE_KEY = Rule(
    id="clip.reference_pose_key",
    comparison=Comparison.EQ,
    unit="keys",
    measured_on=CHANNELS,
)

FPS_GRID = Rule(
    id="clip.fps_grid", comparison=Comparison.LE, unit="frames", measured_on=GRID
)

FPS_GRID_RANGE = Rule(
    id="clip.fps_grid.range", comparison=Comparison.EQ, unit="frames", measured_on=RANGE
)

FLOOR_SNAP = Rule(
    id="clip.floor_snap", comparison=Comparison.LE, unit="meters", measured_on=GROUND
)

STRIDE = Rule(
    id="clip.stride", comparison=Comparison.LE, unit="percent", measured_on=SIZED
)

STRIDE_RATIO = Rule(
    id="clip.stride_ratio", comparison=Comparison.LE, unit="ratio", measured_on=SEGMENT
)

RULES = (
    INTERPOLATION,
    REFERENCE_POSE_KEY,
    FPS_GRID,
    FPS_GRID_RANGE,
    FLOOR_SNAP,
    STRIDE,
    STRIDE_RATIO,
)
"""Every rule the retarget reports, so it asks for one published limit each."""


class Channel(Frozen):
    """One F-curve of the output action, as these two rules read it."""

    data_path: str
    """What the curve drives, such as `pose.bones["Hips"].location`."""
    interpolations: tuple[str, ...]
    """One per key, in the order the curve holds them."""
    extrapolation: str
    frames: tuple[float, ...]
    """Where each key sits, as the curve's own x coordinate."""

    @property
    def straight(self) -> bool:
        """Whether this curve is a straight line held at both ends."""
        return self.extrapolation == CONSTANT and all(
            step == LINEAR for step in self.interpolations
        )


class Defects(Frozen):
    """One bone's two counts. Zero on both is the answer this expects."""

    not_linear: int = 0
    """Channels that are not LINEAR throughout with CONSTANT extrapolation."""
    outside_range: int = 0
    """Keys at frames outside the source's own frame range."""


def defects(channels: Iterable[Channel], frames: range) -> dict[str, Defects]:
    """Both counts, per bone, for every bone the action drives.

    Every bone with a channel gets an entry, including a clean one: a rule
    that goes quiet when it passes cannot be told from a rule that never ran.
    A curve that drives something other than a bone is not counted here,
    because neither rule names it as a subject.
    """
    counted: dict[str, Defects] = {}
    for channel in channels:
        if (bone := bone_from_data_path(channel.data_path)) is None:
            continue
        so_far = counted.get(bone, Defects())
        counted[bone] = Defects(
            not_linear=so_far.not_linear + (not channel.straight),
            outside_range=so_far.outside_range
            + sum(round(frame) not in frames for frame in channel.frames),
        )
    return counted


def counted(channels: Iterable[Channel], frames: range) -> list[Finding]:
    """`clip.interpolation` and `clip.reference_pose_key`, per bone.

    Both count defects, so neither has a tunable limit and neither reads one.
    """
    counts = defects(channels, frames)
    return [
        INTERPOLATION.measured(
            bone,
            count.not_linear,
            f"{bone} has {count.not_linear} channel(s) that are not {LINEAR} "
            f"with {CONSTANT} extrapolation",
        )
        for bone, count in sorted(counts.items())
    ] + [
        REFERENCE_POSE_KEY.measured(
            bone,
            count.outside_range,
            f"{bone} carries {count.outside_range} key(s) outside the "
            f"source's frame range {frames.start}..{frames.stop - 1}",
        )
        for bone, count in sorted(counts.items())
    ]


class Grid(Frozen):
    """The source action's own key times and frame range.

    Kept as plain numbers because the retarget removes the source action
    before it reports: an action left in the file would be exported beside
    ours.
    """

    keys: tuple[float, ...]
    span: tuple[float, float]


def on_the_grid(keys: Iterable[float], rate: int, rule: Rule) -> list[Finding]:
    """`clip.fps_grid`, one finding per key of the source action.

    Per key rather than per worst, because which keys drifted is what says
    whether the rate is wrong or one key is: a wrong rate drifts every key by
    a different amount and leaves the first one alone.
    """
    return [
        rule.measured(
            f"key at frame {key:g}",
            abs(key - round(key)),
            f"the key at {key:g} sits {abs(key - round(key)):g} frames off a "
            f"whole one in a {rate} fps scene",
        )
        for key in sorted(set(keys))
    ]


def whole_range(sampled: range, keys: Iterable[float], rule: Rule) -> Finding:
    """`clip.fps_grid.range`: the sampled frames are the source's own keys.

    The retarget reads whole frames between the two ends of the action, so a
    30 fps clip read at 24 spans 0.8 to 16.8, rounds to 1 to 17 and drops four
    of its 21 frames. That count is the reading. It reads a gap the other way
    too: a source that does not key every frame is one the retarget samples
    between its keys.
    """
    lost = abs(len(sampled) - len(set(keys)))
    return rule.measured(
        f"frames {sampled.start}..{sampled.stop - 1}",
        float(lost),
        f"the retarget samples {len(sampled)} frame(s) and the source has "
        f"{len(set(keys))} key time(s)",
    )


class Ground(Frozen):
    """One sole point's world height at one frame of the clip."""

    bone: str
    frame: int
    height: float
    """Meters up, in Blender Z-up world space. Zero is the floor: the rig's
    rest pose puts every sole point on it by construction."""


def ground_from(
    paths: Mapping[str, Sequence[Vec3]], frames: Sequence[int]
) -> list[Ground]:
    """One record per sole point per frame, out of what Blender read.

    The sole and not the joint: a joint's rest height is only where the
    contact patch is while the foot keeps its rest pitch, and a cross-rig fit
    matches the source's pitch instead. `clip.foot_contact.penetration` reads
    the same two points, so standing on the floor means one thing.
    """
    return [
        Ground(bone=bone, frame=frame, height=point[2])
        for bone, path in sorted(paths.items())
        for frame, point in zip(frames, path, strict=True)
    ]


def lowest(ground: Sequence[Ground]) -> Ground | None:
    """The lowest any sole point gets over the clip.

    None when the source drives no foot at all, which `on_the_floor` reports
    as a measurement that does not exist rather than as a floor already
    reached.
    """
    return min(ground, key=lambda at: at.height) if ground else None


def floor_lift(ground: Sequence[Ground]) -> float:
    """How far up the clip must move to stand on the floor.

    The whole clip and not one frame: a walk that never lifts its right foot
    is still standing on the ground it plants its left one on.
    """
    low = lowest(ground)
    return 0.0 if low is None else -low.height


def on_the_floor(ground: Sequence[Ground], rule: Rule) -> Finding:
    """`clip.floor_snap`: what is left under the lowest sole once it is
    lifted."""
    low = lowest(ground)
    if low is None:
        return rule.undefined(
            "the whole clip",
            "this clip drives no foot, so it has no sole to be read against",
        )
    return rule.measured(
        f"{low.bone} at frame {low.frame}",
        abs(low.height),
        f"{low.bone} at frame {low.frame} is the lowest any sole point of "
        f"the clip gets, {low.height:.4f} m from the floor the snap aims at",
    )


def stride(
    name: str, fitted: float, source: float, ratio: float, travels: bool, rule: Rule
) -> Finding:
    """`clip.stride`: how far the fit travels against how far its source did.

    Relative, so the same 2 percent means the same thing on a 0.4 m shuffle
    and a 2.3 m strafe. A clip the library declares in place has no travel to
    be sized, so the declaration switches the rule off.
    """
    if not travels:
        return rule.skipped(
            name,
            f"the library declares travels: false, so {name} has no source "
            f"travel to be sized against",
        )
    wanted = source * ratio
    if wanted <= 0.0:
        return rule.undefined(
            name,
            f"the source of {name} travels {source:.4f} m, so there is no "
            f"travel for the femur ratio to size",
        )
    return rule.measured(
        name,
        abs(fitted - wanted) / wanted * 100.0,
        f"{name} travels {fitted:.4f} m against the {wanted:.4f} m its "
        f"source's {source:.4f} m comes to at a femur ratio of {ratio:.4f}",
    )


def stride_ratio(
    name: str, ours: float, theirs: float, ratio: float, rule: Rule
) -> Finding:
    """`clip.stride_ratio`: the femur ratio `clip.stride` is read against.

    Records rather than gates: a Mixamo rig and a refit of our own clip are
    both right, and `translation_scale` has already refused a ratio no two
    rigs of one species could have. See the Test Plan row for the readings.
    """
    return rule.measured(
        name,
        ratio,
        f"our stride segment is {ours:.4f} m against the source's "
        f"{theirs:.4f} m, so every length of {name} is sized by {ratio:.4f}",
    )


class Fit(Frozen):
    """Where a fitted clip ended up, as the three placement rules read it.

    Everything here is read back off the pose after the keys are written:
    where a foot lands and how far a root travels are not things the keys say
    on their own.
    """

    name: str
    ground: tuple[Ground, ...]
    """Every sole point of every foot, frame by frame."""
    travel: tuple[float, float]
    """How far the fit's root and the source's hips each got, horizontally."""
    segment: tuple[float, float]
    """Our stride segment's length and the source's."""
    ratio: float
    """The femur ratio every location key was sized by."""
    travels: bool
    """What the library declares about the source, which picks whether
    `clip.stride` measures or reports itself switched off."""


def placed(fit: Fit, limits: dict[str, float]) -> list[Finding]:
    """`clip.floor_snap`, `clip.stride` and `clip.stride_ratio`, on one fit."""
    return [
        on_the_floor(fit.ground, FLOOR_SNAP.at(limits)),
        stride(
            fit.name,
            fit.travel[0],
            fit.travel[1],
            fit.ratio,
            fit.travels,
            STRIDE.at(limits),
        ),
        stride_ratio(
            fit.name,
            fit.segment[0],
            fit.segment[1],
            fit.ratio,
            STRIDE_RATIO.at(limits),
        ),
    ]


class Frame(Frozen):
    """Where the source's bones were at one instant of the clip."""

    seconds: float
    """Time from the clip's own first frame. The output GLB stores key times
    in seconds too, so the two align on a quantity neither side indexes."""
    rotations: dict[str, Quat]
    """Role to that bone's world rotation, in Blender Z-up world space."""
    joints: dict[str, Vec3]
    """Role to that bone's world head, in meters, in the same space. No
    rotation rule sees a joint chain, which is the blind spot that shipped a
    hunched idle, and `clip.posture` holds the fit's own joints to these."""


class SourceMotion(Frozen):
    """The vendor clip's own world orientations, as the gate reads them.

    `clip.swing` and `clip.twist` are absolute rules: they measure the
    delivered GLB against the file the motion was bought in. That file is an
    FBX, the Rust `gltf` reader cannot open one, and CI has no Blender. This
    record is the bridge. Blender writes it here, beside the report, and
    `crates/xtask-art/src/check/clip.rs` reads it with the output GLB.

    Two lengths ride along, because `clip.stride` and `clip.stride_ratio`
    measure the fit against the same unreadable file and neither of them is a
    rotation.
    """

    rest: dict[str, Quat]
    """Role to that bone's rest world rotation, after the re-roll onto its own
    children. `clip.twist` measures each rig against its own rest, which is
    what makes the 174 degrees of convention difference cancel instead of
    failing every correct clip."""
    frames: tuple[Frame, ...]
    travel: float
    """How far the source's own root got from where it started, horizontally,
    in meters. `clip.stride` sizes this by the femur ratio and holds the
    delivered clip to it."""
    stride_segment: float
    """The source rig's two `stride_segment` joints at rest, apart, in meters.
    `clip.stride_ratio` reads it against ours, which Rust takes off the rig GLB
    rather than from here, so the two sides of the ratio have two readers."""

    @model_validator(mode="after")
    def every_frame_must_carry_every_role_the_rest_pose_has(self) -> "SourceMotion":
        """A role missing from one frame would leave that frame unmeasured,
        and a rule with a hole in it is the failure this pipeline shipped."""
        if not self.frames:
            raise ValueError("a source motion with no frame measures nothing")
        for frame in self.frames:
            for what, held in (
                ("rotations", frame.rotations),
                ("joints", frame.joints),
            ):
                if set(held) != set(self.rest):
                    odd = sorted(set(held) ^ set(self.rest))
                    raise ValueError(
                        f"the {what} of the frame at {frame.seconds} s "
                        f"disagrees with the rest pose about {odd}"
                    )
        return self

    def write(self, path: pathlib.Path) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(self.model_dump_json(indent=2) + "\n")


def source_motion(
    rest: dict[str, Mat4],
    frames: dict[int, dict[str, Mat4]],
    fps: float,
    fps_base: float,
    travel: float,
    stride_segment: float,
) -> SourceMotion:
    """The sidecar, from the world matrices the transfer already read.

    `fps` and `fps_base` are the scene's two rate fields, and Blender's real
    rate is `fps / fps_base`: a 29.97 scene stores 30 and 1.001. That rate is
    the source clip's own, and it is what turns a frame number into the same
    seconds the exported GLB carries, so both sides align without either being
    told the other's numbering.

    A `fps_base` other than 1 is refused. Nothing in the library declares a
    fractional rate, `source_frames` already refuses a clip whose frames are
    not whole on the scene's grid, and an unmeasured rate is exactly where
    precise wrong numbers come from.
    """
    if fps <= 0.0 or fps_base <= 0.0:
        raise ValueError(f"a clip rate is positive, got {fps} over {fps_base}")
    rate = fps / fps_base
    if fps_base != 1.0:
        raise ValueError(
            f"the scene runs at {rate:.3f} fps, stored as {fps} over {fps_base}. "
            f"No clip here has been measured on a fractional rate, so it is "
            f"refused rather than fitted"
        )
    if not frames:
        raise ValueError("a source motion with no frame measures nothing")
    first = min(frames)
    return SourceMotion(
        travel=travel,
        stride_segment=stride_segment,
        rest={role: mat_rotation(matrix, role) for role, matrix in rest.items()},
        frames=tuple(
            Frame(
                seconds=(frame - first) / rate,
                rotations={
                    role: mat_rotation(matrix, role) for role, matrix in world.items()
                },
                joints={
                    role: mat_translation(matrix) for role, matrix in world.items()
                },
            )
            for frame, world in sorted(frames.items())
        ),
    )
