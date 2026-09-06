"""Pure geometry and scheduling for the sprite bake.

Everything here is deliberately free of `bpy`. Blender's API only exists inside
Blender, so anything importing it cannot be unit tested, keeping the maths in
its own module is what makes the parts that have historically broken (camera
framing, frame sampling, the forearm roll's mirror) testable at all.

`bake_sprites` supplies the numbers it reads out of Blender; this module
decides what to do with them.
"""

import math
from collections.abc import Iterable, Sequence

from findings import Comparison, Finding, Rule
from pydantic import BaseModel, ConfigDict, Field, model_validator

# Direction 0 faces the camera. `direction_rotation` turns the model by a
# negative Z angle per index, which reads as clockwise on screen. So the ring
# runs south, south-west, west, and on round. The other naming mirrors every
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
    forearm_roll: float = 0.0

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


def forearm_roll_sign(bone_name: str) -> float:
    """Which way a forearm bone rolls, so both arms turn palms-inward.

    Covers both common rig conventions: `LeftForeArm` and `forearm.L`.
    """
    lowered = bone_name.lower()
    is_left = "left" in lowered or lowered.endswith((".l", "_l"))
    return 1.0 if is_left else -1.0


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


SAME_BODY = 1e-4
"""How far two rest heights may sit apart and still be one rig, as a ratio.

Calibrated: the three committed clips read **1.227e-6** against the committed
character, which is the `f32` a GLB stores a joint position in, and a clip
bought on another rig is percent-scale out. This sits 81x over that noise and
1,000x under the 12 percent a Mixamo femur differs by."""


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
