"""Pure geometry and scheduling for the sprite bake.

Everything here is deliberately free of `bpy`. Blender's API only exists inside
Blender, so anything importing it cannot be unit tested, keeping the maths in
its own module is what makes the parts that have historically broken (camera
framing, frame sampling, the forearm roll's mirror) testable at all.

`bake_sprites` supplies the numbers it reads out of Blender; this module
decides what to do with them.
"""

import math
import tomllib
from collections.abc import Iterable

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
Vec4 = tuple[float, float, float, float]
"""A quaternion, in Blender's w, x, y, z order."""


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


def bare_bone_name(name: str) -> str:
    """`mixamorig:LeftArm` -> `leftarm`: no namespace, no case.

    The form two rigs' bone names are compared in.
    """
    return name.rsplit(":", 1)[-1].lower()


def unfilled_roles(names: dict[str, str], bones: Iterable[str]) -> list[str]:
    """Roles this rig has no bone for, so a retarget cannot drive them."""
    available = {bare_bone_name(bone) for bone in bones}
    return sorted(
        role for role, bone in names.items() if bare_bone_name(bone) not in available
    )


OTHER_READERS = frozenset({"profile", "aim_table"})
"""Tables of a skeleton file that belong to another reader: `[profile]` to the
Rust rig gates, `[aim_table]` to the transfer. Named one by one, so a
misspelled table is still refused rather than dropped."""


class SkeletonRoles(Frozen):
    """Which bone name fills each anatomical role, one map per convention.

    Read from `art/skeletons/<skeleton>.toml`. Matching by role rather than by
    name is what stops this rig's `Spine`, its highest, taking the motion of
    Mixamo's `Spine`, its lowest.
    """

    canonical: str
    """The convention the canonical rig itself is named in."""
    conventions: dict[str, dict[str, str]]
    """Convention name to role to bone name."""

    @classmethod
    def parse(cls, text: str) -> "SkeletonRoles":
        """Reads the role tables out of a skeleton file."""
        tables = tomllib.loads(text)
        return cls(**{k: v for k, v in tables.items() if k not in OTHER_READERS})

    @model_validator(mode="after")
    def the_canonical_convention_must_be_declared(self) -> "SkeletonRoles":
        if self.canonical not in self.conventions:
            known = sorted(self.conventions)
            raise ValueError(
                f"canonical convention {self.canonical!r} is not in {known}"
            )
        return self

    @model_validator(mode="after")
    def every_convention_must_fill_every_role(self) -> "SkeletonRoles":
        """A role only one convention knows about cannot be retargeted."""
        roles = set(self.conventions[self.canonical])
        for name, convention in self.conventions.items():
            if difference := roles.symmetric_difference(convention):
                raise ValueError(
                    f"convention {name!r} disagrees about {sorted(difference)}, "
                    f"every convention must fill the same roles"
                )
        return self

    def convention(self, name: str) -> dict[str, str]:
        """One convention's role to bone name map."""
        if (convention := self.conventions.get(name)) is None:
            known = sorted(self.conventions)
            raise ValueError(f"unknown bone naming convention {name!r}, known: {known}")
        return convention

    def bone_map(self, convention: str) -> dict[str, str]:
        """Bare source bone name to canonical bone name, paired by role."""
        canonical = self.conventions[self.canonical]
        return {
            bare_bone_name(bone): canonical[role]
            for role, bone in self.convention(convention).items()
        }


# How far two rigs' bind poses may differ before an animation is refused.
# Meshy reconstructs each character separately, so identical rigs still land a
# degree or two apart; 64 degrees apart is a T-pose against an A-pose.
BIND_POSE_TOLERANCE_DEG = 15.0


def bone_direction_angle(a: Vec3, b: Vec3) -> float:
    """Angle in degrees between two bone directions."""
    dot = sum(x * y for x, y in zip(a, b, strict=True))
    length = math.sqrt(sum(x * x for x in a)) * math.sqrt(sum(x * x for x in b))
    if length == 0.0:
        return 0.0
    return math.degrees(math.acos(max(-1.0, min(1.0, dot / length))))


def bind_pose_mismatch(
    character: dict[str, Vec3],
    animation: dict[str, Vec3],
    tolerance_deg: float = BIND_POSE_TOLERANCE_DEG,
) -> list[tuple[str, float]]:
    """Bones whose rest direction differs too much between two rigs.

    An action holds each bone's rotation *relative to its rest pose*, so
    applying it to a rig in a different rest pose adds that difference to every
    joint. Worst case the character flails.
    """
    off = []
    for name in sorted(set(character) & set(animation)):
        angle = bone_direction_angle(character[name], animation[name])
        if angle > tolerance_deg:
            off.append((name, angle))
    off.sort(key=lambda pair: pair[1], reverse=True)
    return off


def rest_height(points: Iterable[Vec3]) -> float:
    """Vertical span of an armature's rest bones, in world units.

    Rest positions rather than a posed mesh, because an animation file carries
    only its armature and a carrier triangle. Nothing to measure reads as zero,
    which `translation_scale` refuses with a message that says why.
    """
    heights = [point[2] for point in points]
    return max(heights) - min(heights) if heights else 0.0


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


# How far a clip's first and last pose may differ before it is worth a look.
# Tighter than the bind pose tolerance: a gait either closes or it hitches.
LOOP_TOLERANCE_DEG = 2.0


def rotation_angle(a: Vec4, b: Vec4) -> float:
    """Angle in degrees between two rotations.

    A quaternion and its negation are the same rotation, hence the absolute
    value: without it, half the comparisons read as a full turn apart.
    """
    dot = abs(sum(x * y for x, y in zip(a, b, strict=True)))
    length = math.sqrt(sum(x * x for x in a)) * math.sqrt(sum(x * x for x in b))
    if length == 0.0:
        return 0.0
    return math.degrees(2.0 * math.acos(min(1.0, dot / length)))


def loop_mismatch(
    first: dict[str, Vec4],
    last: dict[str, Vec4],
    tolerance_deg: float = LOOP_TOLERANCE_DEG,
) -> list[tuple[str, float]]:
    """Bones whose first and last pose differ, worst first.

    A looping clip has to be one whole cycle, or playback jumps back to the
    start pose once a loop. Advisory: whether a clip is a cycle is a judgement
    call, so this reports and never refuses.
    """
    off = [
        (name, angle)
        for name in sorted(set(first) & set(last))
        if (angle := rotation_angle(first[name], last[name])) > tolerance_deg
    ]
    off.sort(key=lambda pair: pair[1], reverse=True)
    return off
