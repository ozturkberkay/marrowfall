"""Pure geometry and scheduling for the sprite bake.

Everything here is deliberately free of `bpy`. Blender's API only exists inside
Blender, so anything importing it cannot be unit tested, keeping the math in
its own module is what makes the parts that have historically broken (camera
framing, frame sampling, the forearm roll's mirror) testable at all.

`bake_sprites` supplies the numbers it reads out of Blender; this module
decides what to do with them.
"""

import math
import pathlib
from collections.abc import Iterable, Sequence

from findings import Comparison, Finding, Rule
from pydantic import BaseModel, ConfigDict, Field, model_validator

# Direction 0 faces the camera. `direction_rotation` turns the model by a
# negative Z angle per index, which reads as clockwise on screen. So the ring
# runs south, south-west, west, and on around. The other naming mirrors every
# diagonal and swaps east with west, and it leaves south and north looking
# correct. That is what makes the mistake easy to miss.
#
# Past sixteen the compass runs out of names, so that ring is numbered from the
# same stop in the same order. Must match `direction_names` in
# `crates/xtask-art/src/pack.rs`, which a test cross-checks.
DIRECTION_NAMES: dict[int, list[str]] = {
    4: ["s", "w", "n", "e"],
    8: ["s", "sw", "w", "nw", "n", "ne", "e", "se"],
    16: [
        "s",
        "ssw",
        "sw",
        "wsw",
        "w",
        "wnw",
        "nw",
        "nnw",
        "n",
        "nne",
        "ne",
        "ene",
        "e",
        "ese",
        "se",
        "sse",
    ],
    32: [f"{index:02d}" for index in range(32)],
}

# Elevation of the camera above the horizon, in degrees. Must match the tile
# projection: atan(0.5) = 26.57 for a true 2:1 diamond, but characters read
# better slightly higher, matching Diablo II's own camera.
CAMERA_ELEVATION_DEG = 35.0

# Key light azimuth: 315 = upper-left in screen space.
KEY_LIGHT_AZIMUTH_DEG = 315.0
KEY_LIGHT_ELEVATION_DEG = 60.0

# Headroom around the widest animated pose. Without it, limbs at full
# extension clip against the edge of the render canvas.
FRAMING_MARGIN = 1.08

# What share of a bare mesh's vertices the forearm close-up frames: the ones
# furthest from X = 0. In any arms-out pose those are the hands and forearms.
FOREARM_SHARE = 0.1

# Elevation of the forearm close-up. The one angle that shows the top surface
# of a forearm in an A-pose and in a T-pose alike, which is the surface a
# generator has to invent when the concept views hide it.
FOREARM_ELEVATION_DEG = 45.0

Vec3 = tuple[float, float, float]


class Frozen(BaseModel):
    """Immutable, and rejects fields that are not declared."""

    model_config = ConfigDict(frozen=True, extra="forbid")


class BakeSettings(Frozen):
    """Validated bake parameters, straight off the command line."""

    directions: int = 8
    # Sprite rate per animation name. Rates live in the animation library
    # because an idle and a run need very different ones.
    fps: dict[str, int] = Field(default_factory=dict)
    size: int = Field(default=256, ge=16)
    trim_start: float = Field(default=0.0, ge=0.0, lt=1.0)

    @model_validator(mode="after")
    def every_rate_must_be_sane(self) -> "BakeSettings":
        for name, rate in self.fps.items():
            if not 1 <= rate <= 60:
                raise ValueError(f"{name} fps must be in 1..=60, got {rate}")
        return self

    @model_validator(mode="after")
    def directions_must_be_a_known_ring(self) -> "BakeSettings":
        if self.directions not in DIRECTION_NAMES:
            known = sorted(DIRECTION_NAMES)
            raise ValueError(
                f"directions must be one of {known}, got {self.directions}"
            )
        return self

    @property
    def direction_names(self) -> list[str]:
        return DIRECTION_NAMES[self.directions]


class Bounds(Frozen):
    """World-space corners of a mesh set in one pose."""

    lo: Vec3
    hi: Vec3

    @property
    def size(self) -> Vec3:
        return (
            self.hi[0] - self.lo[0],
            self.hi[1] - self.lo[1],
            self.hi[2] - self.lo[2],
        )

    @property
    def height(self) -> float:
        return self.hi[2] - self.lo[2]

    @property
    def center(self) -> Vec3:
        return (
            (self.lo[0] + self.hi[0]) / 2,
            (self.lo[1] + self.hi[1]) / 2,
            (self.lo[2] + self.hi[2]) / 2,
        )

    @property
    def diagonal(self) -> float:
        return math.dist(self.lo, self.hi)


class Framing(Frozen):
    """What the camera has to cover, measured across every animated pose.

    Shared by all animations on purpose. Framing per animation would give each
    one its own world-to-pixel ratio, so the character would render smaller in
    a wide-reaching run than in an idle.
    """

    axis: Vec3 = (0.0, 0.0, 0.0)
    """Vertical axis the character spins about for the direction ring."""
    lo_z: float
    hi_z: float
    radius: float = Field(ge=0.0)
    """Radius swept about `axis`, what must fit in every facing."""

    @model_validator(mode="after")
    def span_must_be_positive(self) -> "Framing":
        if self.hi_z <= self.lo_z:
            raise ValueError(f"empty vertical span: lo_z={self.lo_z} hi_z={self.hi_z}")
        return self

    @property
    def height(self) -> float:
        return self.hi_z - self.lo_z

    @property
    def footprint(self) -> float:
        return 2.0 * self.radius

    @property
    def center(self) -> Vec3:
        return (self.axis[0], self.axis[1], (self.lo_z + self.hi_z) / 2)

    @property
    def ortho_scale(self) -> float:
        """Orthographic width the camera must cover.

        A camera tilted by `elevation` projects both the subject's height and
        its depth onto the vertical axis of the image, so the vertical span
        needed is `height*cos(e) + depth*sin(e)`, not the height alone. Sizing
        from height alone is what let extended poses clip against the top and
        bottom edges.
        """
        elevation = math.radians(CAMERA_ELEVATION_DEG)
        screen_height = self.height * math.cos(elevation) + self.footprint * math.sin(
            elevation
        )
        # Square canvas, so the scale must cover the larger screen axis.
        return max(self.footprint, screen_height) * FRAMING_MARGIN

    @property
    def camera_location(self) -> Vec3:
        elevation = math.radians(CAMERA_ELEVATION_DEG)
        distance = max(self.height * 4.0, 1.0)
        center = self.center
        return (
            center[0],
            center[1] - distance * math.cos(elevation),
            center[2] + distance * math.sin(elevation),
        )

    @property
    def camera_rotation(self) -> Vec3:
        """Blender cameras look down local -Z.

        A rot_x of 90 degrees looks horizontally along +Y, so subtracting the
        elevation tilts it downward onto the subject.
        """
        return (math.radians(90.0) - math.radians(CAMERA_ELEVATION_DEG), 0.0, 0.0)

    def merged(self, other: "Framing") -> "Framing":
        """The framing that covers both. Used to fold one animation into the rest."""
        return Framing(
            axis=self.axis,
            lo_z=min(self.lo_z, other.lo_z),
            hi_z=max(self.hi_z, other.hi_z),
            radius=max(self.radius, other.radius),
        )


def key_light_rotation() -> Vec3:
    """Euler rotation for the key light.

    World-fixed: because the character rotates rather than the camera, this
    keeps the light falling from screen upper-left in every facing.
    """
    return (
        math.radians(90.0) - math.radians(KEY_LIGHT_ELEVATION_DEG),
        0.0,
        math.radians(KEY_LIGHT_AZIMUTH_DEG),
    )


class Still(Frozen):
    """One orthographic view of a mesh nothing animates.

    The model contact sheet is made of these: what a human reads to answer
    whether the generator invented surface it had no reference for.
    """

    name: str
    azimuth_degrees: float
    """Turn about the world up axis. The character faces -Y, so 0 is his
    front and 90 is his own left."""
    elevation_degrees: float
    box: Bounds
    """What the camera must cover."""

    @model_validator(mode="after")
    def the_box_must_have_size(self) -> "Still":
        if self.box.diagonal <= 0.0:
            raise ValueError(f"{self.name}: the box to frame is a single point")
        return self

    @property
    def ortho_scale(self) -> float:
        """The box's own diagonal, which covers it from any angle.

        Framing each azimuth exactly would need the projected extents. The
        diagonal is the one number that cannot clip, and on a standing body
        it sits a few percent over the height that dominates it.
        """
        return self.box.diagonal

    @property
    def camera_location(self) -> Vec3:
        azimuth = math.radians(self.azimuth_degrees)
        elevation = math.radians(self.elevation_degrees)
        distance = max(self.ortho_scale * 4.0, 1.0)
        center = self.box.center
        return (
            center[0] + distance * math.cos(elevation) * math.sin(azimuth),
            center[1] - distance * math.cos(elevation) * math.cos(azimuth),
            center[2] + distance * math.sin(elevation),
        )

    @property
    def camera_rotation(self) -> Vec3:
        """Blender cameras look down local -Z, so an unturned one at rot_x of
        90 degrees looks along +Y, which is the front view."""
        return (
            math.radians(90.0 - self.elevation_degrees),
            0.0,
            math.radians(self.azimuth_degrees),
        )

    @property
    def key_light_rotation(self) -> Vec3:
        """The bake's key light turned to this view's own azimuth, so every
        still is lit from its own screen upper-left and not from behind."""
        pitch, _, roll = key_light_rotation()
        return (pitch, 0.0, roll + math.radians(self.azimuth_degrees))


def outermost_box(points: Sequence[Vec3], share: float) -> Bounds:
    """The box around the `share` of points furthest from X = 0.

    In any arms-out pose those are the hands and the forearms. Read off the
    geometry because there is no joint to read instead: the mesh the model
    stage downloads carries no skeleton at all.
    """
    if not points:
        raise ValueError("no points to frame")
    if not 0.0 < share <= 1.0:
        raise ValueError(f"share must be in 0.0 < share <= 1.0, got {share}")
    ranked = sorted(points, key=lambda point: abs(point[0]), reverse=True)
    kept = ranked[: max(1, math.ceil(len(ranked) * share))]
    return Bounds(
        lo=tuple(min(point[axis] for point in kept) for axis in range(3)),
        hi=tuple(max(point[axis] for point in kept) for axis in range(3)),
    )


def mesh_stills(body: Bounds, points: Sequence[Vec3]) -> list[Still]:
    """The five views of the model contact sheet.

    Four elevations of the whole body, then the arms alone from above the
    front, which is where a forearm's top surface is.
    """
    arms = outermost_box(points, FOREARM_SHARE)
    flat = [("front", 0.0), ("back", 180.0), ("left", 90.0), ("right", 270.0)]
    return [
        Still(name=name, azimuth_degrees=azimuth, elevation_degrees=0.0, box=body)
        for name, azimuth in flat
    ] + [
        Still(
            name="forearms",
            azimuth_degrees=0.0,
            elevation_degrees=FOREARM_ELEVATION_DEG,
            box=arms,
        )
    ]


def direction_rotation(index: int, count: int) -> float:
    """Z rotation, in radians, that turns the character to face `index`.

    Index 0 is unrotated, which is the character facing the camera: Meshy
    exports him facing -Y and the camera sits there. Verified by rendering the
    ring, not derived, the geometry is too close to call either way.

    The stops are evenly spaced in the *world*, so every wedge is the same
    number of world degrees. The 2:1 projection then makes them uneven on
    screen, which is honest: gameplay happens in the world, and the answer to a
    wedge being too coarse is more stops, not a different arrangement of the
    same ones.
    """
    return -(2.0 * math.pi / count) * index


def frame_filename(animation: str, direction: str, index: int) -> str:
    """`cargo art`'s packer parses these names, so the shape is load-bearing."""
    return f"{animation}_{direction}_{index:02d}.png"


def sampled_frames(
    frame_start: float,
    frame_end: float,
    scene_fps: int,
    fps: int,
    trim_start: float,
) -> list[int]:
    """Frames to render, sampled at `fps` across an action's duration.

    The count follows from the action's length rather than being fixed, so a
    short animation is not stretched and a long one is not crushed. The final
    frame is excluded: for a loop it duplicates the first.
    """
    span = frame_end - frame_start
    start = frame_start + span * trim_start
    span = frame_end - start

    seconds = max(span / (scene_fps or 24), 0.0)
    count = max(round(seconds * fps), 1)
    return [round(start + span * i / count) for i in range(count)]


def is_forearm(bone_name: str) -> bool:
    lowered = bone_name.lower()
    return "forearm" in lowered or "lowerarm" in lowered


def bone_from_data_path(data_path: str) -> str | None:
    """The bone an F-curve drives, or None if the curve is not bone-scoped."""
    if not data_path.startswith('pose.bones["'):
        return None
    parts = data_path.split('"')
    return parts[1] if len(parts) > 1 else None


def missing_bones(animated: set[str], available: set[str]) -> list[str]:
    """Bones an action drives that the armature does not have.

    A non-empty result means the animation and the character came from
    different rigs, which produces a silently frozen or mangled bake.
    """
    return sorted(animated - available)


def rest_height(points: Iterable[Vec3]) -> float:
    """Vertical span of an armature's rest bones, in world units.

    Rest positions rather than a posed mesh, because an animation file carries
    only its armature and a carrier triangle. Nothing to measure reads as zero,
    which `translation_scale` refuses with a message that says why.
    """
    heights = [point[2] for point in points]
    return max(heights) - min(heights) if heights else 0.0


PINNED = (
    "the root bone's world head against its first frame, per horizontal axis, "
    "after strip_root_motion"
)

KEPT = (
    "the root bone's world head against its first frame, on the up axis, "
    "after strip_root_motion"
)

ROOT_TRAVEL = Rule(
    id="clip.root_travel",
    comparison=Comparison.LE,
    unit="meters",
    measured_on=PINNED,
)
"""What is left on the two axes the strip pins."""

ROOT_BOB = Rule(
    id="clip.root_bob",
    comparison=Comparison.LE,
    unit="meters",
    measured_on=KEPT,
)
"""And what is left on the one it keeps, which is the bob."""

BAKE_RULES = (ROOT_TRAVEL, ROOT_BOB)
"""Every rule the bake reports, so it asks for one published limit each.
Published in `crates/xtask-art/src/check/clip.rs`, which is what
`--list-rules` prints."""

AXES = ("x", "y", "z")
"""What a per-axis finding names itself, in the order a Vec3 holds them."""

UP_AXIS = 2
"""Which component is up, in the Blender world space these rules read.
`[profile] up_axis` says the same thing for the rig gates."""


def pin_horizontally(path: Sequence[Vec3]) -> list[Vec3]:
    """Every point moved back onto the first one's horizontal position, each
    keeping its own height.

    World points, because the root's own channels are not world axes: on this
    rig `Hips` local Z is world minus Z tilted 8.9 degrees (fact 5). Height is
    kept because a bob is animation, and `clip.root_bob` reads it.
    """
    if not path:
        return []
    first = path[0]
    return [(first[0], first[1], point[2]) for point in path]


def worst_axis_travel(path: Sequence[Vec3]) -> Vec3:
    """How far each axis gets from the first point, at its worst.

    A maximum over the whole path rather than the difference between its
    ends: an endpoint difference cancels a symmetric excursion, and a clip
    that slides half a meter out and back inflates the crop for every frame
    of every direction.
    """
    if not path:
        raise ValueError("a clip with no frame has no travel to measure")
    worst = tuple(
        max(abs(point[axis] - path[0][axis]) for point in path) for axis in range(3)
    )
    return (worst[0], worst[1], worst[2])


def root_channel_fault(bone: str, action: str, keys: Sequence[int]) -> str | None:
    """Why a root bone's location channels cannot be pinned, or None.

    `keys` is one key count per location channel found. A world-space pin
    reads all three channels of one key together, so anything but three
    channels of the same non-zero length is a pin against a coordinate that
    does not exist. No channel at all is not a fault: that bone is not keyed.
    """
    if not keys:
        return None
    if len(keys) != 3:
        return (
            f"{bone} in {action!r} has {len(keys)} of 3 location channels, and "
            f"a world-space pin reads all three of them together"
        )
    counts = sorted(set(keys))
    if len(counts) != 1 or counts == [0]:
        return (
            f"the location curves of {bone} in {action!r} hold {counts} keys, "
            f"and a world-space pin reads all three channels of one key together"
        )
    return None


def bake_rule(axis: int) -> Rule:
    """Which of the two bake rules reads one axis of the world."""
    return ROOT_BOB if axis == UP_AXIS else ROOT_TRAVEL


def root_travel(
    name: str, bone: str, path: Sequence[Vec3], limits: dict[str, float]
) -> list[Finding]:
    """Both bake rules, one finding per axis of one clip.

    The two horizontal axes are `clip.root_travel`: `pin_horizontally` puts
    them on the first frame's value, so any reading at all is a residual. The
    up axis is `clip.root_bob`, and it has a limit of its own because what is
    left there is the bob the strip keeps on purpose.
    """
    findings = []
    for index, (axis, worst) in enumerate(
        zip(AXES, worst_axis_travel(path), strict=True)
    ):
        if index == UP_AXIS:
            message = (
                f"{bone} bobs {worst:.4f} m along {axis} over {name}, which "
                f"the strip keeps"
            )
        else:
            message = (
                f"{bone} drifts {worst:.4f} m along {axis} over {name} once "
                f"its horizontal motion is pinned"
            )
        findings.append(
            bake_rule(index).at(limits).measured(f"{name} {axis}", worst, message)
        )
    return findings


def root_kept(name: str, bone: str, limits: dict[str, float]) -> list[Finding]:
    """Both bake rules as skips, for a run that kept the root motion.

    `--keep-root-motion` is a declared flag, so every axis still reports:
    a rule that goes quiet cannot be told from one that never ran.
    """
    findings = []
    for index, axis in enumerate(AXES):
        findings.append(
            bake_rule(index)
            .at(limits)
            .skipped(
                f"{name} {axis}",
                f"--keep-root-motion left {bone} traveling on purpose",
            )
        )
    return findings


SAMPLED = "the frames the bake renders, against the frames the clip's own action keys"

SAMPLED_FRAMES_ARE_KEYS = Rule(
    id="bake.sampled_frames_are_keys",
    comparison=Comparison.EQ,
    unit="frames",
    measured_on=SAMPLED,
)
"""Whether every rendered frame is a pose somebody authored.

There is no divisibility rule between the sprite rate and the clip's own rate,
and none is wanted: what matters is that a rendered frame is an authored key
rather than an interpolation of two."""

PROJECTED = (
    "every joint of the rig at three sampled frames, projected through the "
    "bake camera to whole pixels, against the committed golden, the wider of "
    "the two axes of the worst joint"
)

LANDMARK_GOLDEN = Rule(
    id="bake.landmark_golden",
    comparison=Comparison.LE,
    unit="pixels",
    measured_on=PROJECTED,
)
"""Where every joint landed on screen, against the pixels a human signed off."""

UPDATE_GOLDENS = "MARROWFALL_UPDATE_GOLDENS=1"
"""What a message names as the only thing that rewrites a golden. The Rust
runner reads that variable and passes the flag; no script here reads it."""


def frames_the_action_keys(channels: Iterable[Iterable[float]]) -> set[int]:
    """Every whole frame any channel of an action keys.

    Any channel and not every one of them, which was measured: 149 of the 240
    curves a committed clip carries are a bone's own constant `location` and
    `scale`, and their `f32` noise overlaps the smallest real motion in the
    same file. Correction 2 of T14 in the design has both numbers.

    Whole frames, because `sampled_frames` rounds every sample it returns.
    """
    return {round(frame) for channel in channels for frame in channel}


def unkeyed_frames(sampled: Iterable[int], keyed: set[int]) -> list[int]:
    """The frames the bake renders that nothing authored a pose at."""
    return sorted({frame for frame in sampled if frame not in keyed})


def frames_are_keys(
    name: str,
    sampled: Sequence[int],
    channels: Iterable[Iterable[float]],
    limits: dict[str, float],
) -> Finding:
    """`bake.sampled_frames_are_keys` for one clip."""
    keyed = frames_the_action_keys(channels)
    unkeyed = unkeyed_frames(sampled, keyed)
    return SAMPLED_FRAMES_ARE_KEYS.at(limits).measured(
        name,
        len(unkeyed),
        f"{name} renders {len(sampled)} frame(s) of the {len(keyed)} its "
        f"action keys" + (f", and {unkeyed} are keyed by nothing" if unkeyed else ""),
    )


class Camera(Frozen):
    """The bake camera as a pixel mapper.

    Orthographic, so a projection is a scale and never a divide: the camera's
    own right and up directions carry a world point across the canvas, and
    `ortho_scale` is how much world that square canvas covers.
    """

    location: Vec3
    right: Vec3
    up: Vec3
    ortho_scale: float = Field(gt=0.0)
    size: int = Field(ge=16)


class Landmark(Frozen):
    """Where one joint landed, in one sampled frame of one direction."""

    frame: int
    bone: str
    x: int
    y: int

    @property
    def at(self) -> tuple[int, str]:
        """What names this row, which is what a golden is matched on."""
        return (self.frame, self.bone)


def project(point: Vec3, camera: Camera) -> tuple[int, int]:
    """One world point as the whole pixel of the rendered frame it lands on.

    Rows count down from the top, the way an image does.
    """
    offset = tuple(point[axis] - camera.location[axis] for axis in range(3))
    across = sum(offset[axis] * camera.right[axis] for axis in range(3))
    up = sum(offset[axis] * camera.up[axis] for axis in range(3))
    return (
        round((0.5 + across / camera.ortho_scale) * camera.size),
        round((0.5 - up / camera.ortho_scale) * camera.size),
    )


def golden_samples(count: int) -> list[int]:
    """Which sampled frames a golden records: the first, the middle and the
    last.

    Three of them, and not every frame: the full form is about 37,000 lines
    and would be rubber-stamped. Deduplicated, so a clip of one or two frames
    records what it has.
    """
    return sorted({0, count // 2, count - 1}) if count > 0 else []


GOLDEN_ROW = "{frame:<7}{bone:<16}{x:>5}{y:>5}"
"""One landmark per line, wide enough for `RightShoulder` and a four digit
canvas."""

GOLDEN_HEADER = GOLDEN_ROW.format(frame="frame", bone="bone", x="x", y="y")
"""The one line a golden carries that is not a landmark. Built from the row
format, so a label cannot sit over the wrong field."""


def golden_text(landmarks: Iterable[Landmark]) -> str:
    """A golden file, verbatim."""
    rows = "".join(
        GOLDEN_ROW.format(frame=mark.frame, bone=mark.bone, x=mark.x, y=mark.y) + "\n"
        for mark in landmarks
    )
    return f"{GOLDEN_HEADER}\n{rows}"


def golden_landmarks(text: str) -> list[Landmark]:
    """The landmarks a golden file records, header and blank lines dropped."""
    marks = []
    for line in text.splitlines():
        fields = line.split()
        if len(fields) != 4 or not fields[0].isdecimal():
            continue
        marks.append(
            Landmark(
                frame=int(fields[0]),
                bone=fields[1],
                x=int(fields[2]),
                y=int(fields[3]),
            )
        )
    return marks


def golden_gap(
    measured: Sequence[Landmark], recorded: Sequence[Landmark]
) -> tuple[int, str] | None:
    """How far this run sits from the golden, and which joint is worst.

    `None` when the two do not describe the same joints at the same frames: a
    26 joint pose has no distance from a 24 joint record, and saying it does
    would compare whatever happened to line up.
    """
    theirs = {mark.at: mark for mark in recorded}
    if sorted(theirs) != sorted(mark.at for mark in measured):
        return None
    worst = 0
    where = "every joint is on the pixel the golden records"
    for mark in measured:
        golden = theirs[mark.at]
        apart = max(abs(mark.x - golden.x), abs(mark.y - golden.y))
        if apart > worst:
            worst = apart
            where = (
                f"{mark.bone} at frame {mark.frame} is {apart} px off, "
                f"({mark.x}, {mark.y}) against ({golden.x}, {golden.y})"
            )
    return (worst, where)


def landmark_golden(
    subject: str,
    path: pathlib.Path,
    measured: Sequence[Landmark],
    limits: dict[str, float],
    update: bool,
) -> Finding:
    """`bake.landmark_golden` for one clip in one direction.

    A missing golden is an error and never an auto-accept. With `update` the
    file is rewritten from this run and the rule reports `skipped`: reading a
    golden this run just wrote would be the run agreeing with itself.
    """
    rule = LANDMARK_GOLDEN.at(limits)
    if update:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(golden_text(measured))
        return rule.skipped(
            subject,
            f"{UPDATE_GOLDENS} rewrote {path.name} from this run, so nothing read it",
        )
    if not path.exists():
        return rule.undefined(
            subject,
            f"there is no golden at {path}, so nothing recorded where {subject} "
            f"belongs. {UPDATE_GOLDENS} writes one",
        )
    gap = golden_gap(measured, golden_landmarks(path.read_text()))
    if gap is None:
        return rule.undefined(
            subject,
            f"{path.name} records another set of joints or frames than this "
            f"run projected, so the two have no distance apart. {UPDATE_GOLDENS} "
            f"rewrites it",
        )
    worst, where = gap
    return rule.measured(
        subject,
        worst,
        f"of the {len(measured)} joints of {subject}, {where}",
    )


SAME_BODY = 1e-4
"""How far two rest heights may sit apart and still be one rig, as a ratio.

Calibrated: the five fitted clips read **1.007e-5** against the committed
character, 166.8788 armature units against 166.8805, and a clip bought on
another rig is percent-scale out. This sits 9.9x over that reading and 1,000x
under the 12 percent a Mixamo femur differs by."""


def off_this_body(source: float, target: float) -> float:
    """How far a clip's own rig sits from the character's, as a ratio off 1.

    The retarget sizes every length by the femur, so a clip that reaches the
    bake is already on this body and this reads 0. Anything else arrived
    unfitted, and scaling it here would size it twice.
    """
    return abs(translation_scale(source, target) - 1.0)


def translation_scale(source: float, target: float) -> float:
    """How much to grow a clip's lengths for this character.

    Rotation is proportion independent; `location` is not, it is a length in
    the units of the rig the clip was authored against. A run's vertical bob
    measured for a 1.7 m body is twice too large on a body half that tall.
    """
    if not (math.isfinite(source) and math.isfinite(target)) or source <= 0:
        raise ValueError(f"cannot size a clip from {source} and {target}")
    ratio = target / source
    if not 0.2 <= ratio <= 5.0:
        raise ValueError(f"ratio {ratio:.2f} outside 0.2..5.0, wrong rig?")
    return ratio
