"""What a vendor clip is, measured before anything is fitted to it.

Six rules, all of them at the fetch boundary and all of them published in
`crates/xtask-art/src/check/source.rs`, which is what
`cargo art check --list-rules` prints. This module holds the maths and builds
the Findings; `check_source.py` is the Blender shell that reads an FBX and
hands the numbers over.

Three of the six gate and three record.

- `source.fps_declared` and the travel pair say the file is the file the
  library declared. A clip whose own rate is not the declared one plays at the
  wrong speed for the rest of its life, and a clip whose root motion is not
  what `travels` says is either a walk that never leaves the origin or an
  in-place cycle sliding out of frame.
- `source.wander`, `source.child_axis` and `source.posture` record. Their
  ceilings are set so far out that every reading is information. A vendor
  skeleton is not ours to regenerate: Mixamo's `Neck` axis sits 16.933 degrees
  off the direction to its own `Head`, and after T15 regenerates our rig that
  difference is still there on every Mixamo clip. And how far the hips wander
  on the way is 0.0276 m for a run cycle against 2.3117 for a strafe, 84 times
  further, so no one threshold reads both.

`travels` picks which of the travel pair measures: the other reports
`skipped`. One id carrying two comparisons could not be held to the rule
list, and the list is the one thing between a hand-written finding here and a
limit nobody published.

Free of `bpy`, so it is unit tested with no Blender.
"""

import math
from collections.abc import Sequence

from findings import Comparison, Finding, Rule
from framing import Frozen
from transfer import Mat4, Vec3, length, mat_direction, mat_translation

UP: Vec3 = (0.0, 0.0, 1.0)
"""Blender's up, which `[profile] up_axis` names. Every angle here is read
against it, in Blender Z-up world space, which is the one space the clip
rules already state."""

RATE = "the vendor file's own key spacing, against the rate the library declares"

HIPS = "the vendor file's hips, first frame to last, in Blender Z-up world space"

PATH = (
    "the vendor file's hips at every frame, against the first, horizontally, "
    "in Blender Z-up world space"
)

REST = (
    "the vendor rig's own bone axes at rest, against the direction to each "
    "mapped child, in Blender Z-up world space"
)

POSE = "the vendor rig's joints over the clip, in Blender Z-up world space"

FPS_DECLARED = Rule(
    id="source.fps_declared",
    comparison=Comparison.EQ,
    unit="frames per second",
    measured_on=RATE,
)

TRAVELING = Rule(
    id="source.traveling", comparison=Comparison.GE, unit="meters", measured_on=HIPS
)

IN_PLACE = Rule(
    id="source.in_place", comparison=Comparison.LE, unit="meters", measured_on=HIPS
)

WANDER = Rule(
    id="source.wander", comparison=Comparison.LE, unit="meters", measured_on=PATH
)

CHILD_AXIS = Rule(
    id="source.child_axis", comparison=Comparison.LE, unit="degrees", measured_on=REST
)

POSTURE = Rule(
    id="source.posture", comparison=Comparison.LE, unit="degrees", measured_on=POSE
)

RULES = (FPS_DECLARED, TRAVELING, IN_PLACE, WANDER, CHILD_AXIS, POSTURE)
"""Every rule this module reports, so the shell asks for one limit each."""

POSTURE_READINGS: tuple[tuple[str, str, str], ...] = (
    ("head pitch", "neck", "head"),
    ("spine lean", "hips", "neck"),
    ("left arm swing", "left_arm", "left_forearm"),
    ("right arm swing", "right_arm", "right_forearm"),
)
"""What a clip's posture is reported as: a label and the two joints whose
direction it reads, against straight up.

Joints and not bone axes, which is the whole point. Mixamo's `Head` bone
points 1.0 degrees off vertical while the joint above it leans 34.9, and 34
to 37 degrees forward is the hunch fact 17 records."""


class Posed(Frozen):
    """Where the vendor rig's bones were at one sampled frame."""

    frame: int
    world: dict[str, Mat4]
    """Role to that bone's world transform, in Blender Z-up world space."""


class Clip(Frozen):
    """Everything Blender read out of one vendor file, ready to measure."""

    declared_fps: int
    """What `art/animations/library.ron` says this clip's own rate is."""
    travels: bool
    """What the same file says its hips do."""
    scene_fps: float
    """The rate the file was imported at, which is Blender's real
    `fps / fps_base`."""
    key_frames: tuple[float, ...]
    """Where the file's own keys landed, on the scene's frame grid."""
    hips: tuple[Vec3, ...]
    """The hips' world position at every frame of the clip."""
    axis: Vec3
    """Which of a bone's own axes points at its child, from `[profile]`."""
    rest: dict[str, Mat4]
    """Role to that bone's rest world transform."""
    children: dict[str, str]
    """Role to the role its own axis must point at. `[profile.tails]` chooses
    which child that is, because `hips` has three and only one of them
    continues the body."""
    posture: tuple[Posed, ...]


def rate(scene_fps: float, key_frames: Sequence[float]) -> float | None:
    """The clip's own rate, or None when it has too few keys to have one.

    Read from the key spacing rather than from the scene, so one formula
    covers both containers: an FBX keys whole frames at the scene's rate, and
    a 30 fps glTF read at 24 lands its keys 0.8 frames apart and still reads
    30. The median spacing, because one duplicated key would halve a mean.
    """
    spacings = sorted(
        b - a
        for a, b in zip(
            sorted(set(key_frames)), sorted(set(key_frames))[1:], strict=False
        )
    )
    if not spacings or scene_fps <= 0.0:
        return None
    middle = spacings[len(spacings) // 2]
    return scene_fps / middle if middle > 0.0 else None


def sampled_frames(frames: range) -> tuple[int, ...]:
    """The frames a clip's posture is read at: its two ends and its middle.

    A few frames rather than all of them, because this rule records what the
    motion looks like rather than gating it, and 21 frames of four readings
    is 84 lines nobody reads.
    """
    if not frames:
        raise ValueError("a clip with no frame has no posture to read")
    last = frames[-1]
    return tuple(sorted({frames.start, frames[len(frames) // 2], last}))


def degrees_between(a: Vec3, b: Vec3) -> float | None:
    """The angle between two directions, or None when one of them is not a
    direction at all. Two joints at the same place have no direction between
    them, and reporting 0 there would be a precise wrong number."""
    scale = length(a) * length(b)
    if scale == 0.0:
        return None
    cosine = sum(x * y for x, y in zip(a, b, strict=True)) / scale
    return math.degrees(math.acos(max(-1.0, min(1.0, cosine))))


def travel(hips: tuple[Vec3, ...]) -> float:
    """How far the hips end up from where they started, horizontally.

    Where they end up, not how far they wandered: a run cycle sways its hips
    0.028 m sideways and returns them exactly, and calling that travel would
    declare every in-place clip a traveling one. `wander` reads the excursion
    beside it. Horizontal, because the vertical part is the bob, which fact 5
    measures at 0.043 m, twice this rule's whole threshold.
    """
    if not hips:
        raise ValueError("a clip with no frame has no travel to measure")
    first, last = hips[0], hips[-1]
    return math.hypot(last[0] - first[0], last[1] - first[1])


def wander(hips: tuple[Vec3, ...]) -> float:
    """The furthest the hips get from where they started, horizontally.

    The reading `travel` cancels: a clip that slides out and back ends where
    it began. Nothing downstream can take it instead, because the bake pins
    the horizontal axes onto the first frame before `clip.root_travel` reads.
    """
    if not hips:
        raise ValueError("a clip with no frame has no path to measure")
    first = hips[0]
    return max(math.hypot(x - first[0], y - first[1]) for x, y, _ in hips)


def joint_direction(bone: Mat4, child: Mat4) -> Vec3:
    """From one joint to another, in the space both are given in."""
    head, tail = mat_translation(bone), mat_translation(child)
    return (tail[0] - head[0], tail[1] - head[1], tail[2] - head[2])


def findings(clip: Clip, limits: dict[str, float]) -> list[Finding]:
    """Every rule this module owns, on one vendor file.

    `limits` is the published limit per rule id, which the Rust runner passes
    on argv. No script here opens a skeleton file to find one.
    """
    return [
        _rate(clip, limits),
        *_travel(clip, limits),
        _wander(clip, limits),
        *_child_axis(clip, limits),
        *_posture(clip, limits),
    ]


def _rate(clip: Clip, limits: dict[str, float]) -> Finding:
    """`source.fps_declared`, as the gap between the two rates.

    The gap rather than the rate itself: a published limit is one number for
    every clip, so the disagreement is the only quantity a published zero can
    read. Rounded to a whole rate, because it is a ratio of two floats; the
    message carries the unrounded number and `clip.source_motion` refuses a
    genuinely fractional rate one step later.
    """
    rule = FPS_DECLARED.at(limits)
    measured = rate(clip.scene_fps, clip.key_frames)
    if measured is None:
        return rule.undefined(
            "the whole clip",
            f"the clip has {len(set(clip.key_frames))} distinct key time(s) at "
            f"{clip.scene_fps:g} fps, which sets no spacing to read a rate from",
        )
    return rule.measured(
        "the whole clip",
        abs(round(measured) - clip.declared_fps),
        f"the file runs at {measured:g} fps and the library declares "
        f"{clip.declared_fps}",
    )


def _travel(clip: Clip, limits: dict[str, float]) -> list[Finding]:
    """The travel pair: `travels` picks which one measures."""
    measured, skipped = (TRAVELING, IN_PLACE) if clip.travels else (IN_PLACE, TRAVELING)
    moved = travel(clip.hips)
    return [
        measured.at(limits).measured(
            "the whole clip",
            moved,
            f"the hips end {moved:.4f} m from where they started, and the "
            f"library declares travels: {str(clip.travels).lower()}",
        ),
        skipped.at(limits).skipped(
            "the whole clip",
            f"the library declares travels: {str(clip.travels).lower()}, which "
            f"{measured.id} is the half that reads",
        ),
    ]


def _wander(clip: Clip, limits: dict[str, float]) -> Finding:
    """`source.wander`, which records rather than gates.

    An in-place cycle wanders 0.0276 m and a strafe 2.3117, 84 times further,
    so no one threshold reads both. The ceiling is further than any clip
    travels, and the number is what the reading is for.
    """
    worst = wander(clip.hips)
    return WANDER.at(limits).measured(
        "the whole clip",
        worst,
        f"the hips get {worst:.4f} m from where they started at their furthest",
    )


def _child_axis(clip: Clip, limits: dict[str, float]) -> list[Finding]:
    """`source.child_axis`, one finding per bone with a mapped child."""
    rule = CHILD_AXIS.at(limits)
    return [
        _angle(
            rule,
            role,
            mat_direction(clip.rest[role], clip.axis),
            joint_direction(clip.rest[role], clip.rest[child]),
            f"its own axis is {{off:.3f}} degrees off the direction to {child}",
        )
        for role, child in sorted(clip.children.items())
        if role in clip.rest and child in clip.rest
    ]


def _posture(clip: Clip, limits: dict[str, float]) -> list[Finding]:
    """`source.posture`, one finding per reading per sampled frame."""
    rule = POSTURE.at(limits)
    return [
        _angle(
            rule,
            f"{label} at frame {posed.frame}",
            UP,
            joint_direction(posed.world[role], posed.world[other]),
            f"{label} is {{off:.3f}} degrees from straight up",
        )
        for posed in clip.posture
        for label, role, other in POSTURE_READINGS
        if role in posed.world and other in posed.world
    ]


def _angle(rule: Rule, subject: str, a: Vec3, b: Vec3, message: str) -> Finding:
    """One angle between two directions, or the reason there is none."""
    off = degrees_between(a, b)
    if off is None:
        return rule.undefined(
            subject, f"{subject} has no direction to measure, so no angle exists"
        )
    return rule.measured(subject, off, f"{subject}: {message.format(off=off)}")
