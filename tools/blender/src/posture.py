"""Posture readings of a clip, eleven numbers per frame.

Measures only, no limits and no gates. Every reading is joint to joint,
except the wrist, which uses the hand bone's own axis.

Signs, in Blender Z-up world space with the character facing minus Y:

- Pitch: degrees off straight up, positive leans forward.
- Roll: degrees off level, positive lifts the left side.
- Arm, elbow and wrist angles are never negative.

Free of `bpy`, so it is unit tested without Blender.
"""

from collections.abc import Sequence

from framing import Frozen
from skeleton import SKULL_TOP

# The same angle and the same joint-to-joint step the source gates read, so
# the two cannot mean different things by the word.
from source import degrees_between, joint_direction
from transfer import Mat4, Vec3, dot, mat_direction, mat_translation

UP: Vec3 = (0.0, 0.0, 1.0)
"""Blender's up, which `[profile] up_axis` names."""

DOWN: Vec3 = (0.0, 0.0, -1.0)
"""Where a hanging arm points."""

FORWARD: Vec3 = (0.0, -1.0, 0.0)
"""Which way the character faces, which is what makes a pitch positive."""


class Reading(Frozen):
    """One posture number: what it is called, and how it prints."""

    label: str
    column: str
    """The short name of its column in the per frame grid."""
    decimals: int = 1


READINGS: tuple[Reading, ...] = (
    Reading(label="skull pitch", column="skull"),
    Reading(label="neck pitch", column="neck"),
    Reading(label="spine lean", column="spine"),
    Reading(label="shoulder roll", column="shldr"),
    Reading(label="left upper arm", column="L arm"),
    Reading(label="right upper arm", column="R arm"),
    Reading(label="left elbow bend", column="L elb"),
    Reading(label="right elbow bend", column="R elb"),
    Reading(label="left wrist bend", column="L wrs"),
    Reading(label="right wrist bend", column="R wrs"),
    Reading(label="hips height", column="hips", decimals=3),
)
"""Every reading, in the order they print. One table for the numbers and the
columns both, so a reading cannot be added to one and left out of the other."""

UNITS = "Degrees, and positive leans forward. hips height is meters."

COLUMN = 8
"""How wide one number prints, which is the widest reading plus a gap."""

NAME = 8
"""How wide a column's short name prints."""

LABEL = 18
"""How wide the longest reading's full name prints, plus a gap."""


class Posed(Frozen):
    """Where one body was at one frame."""

    frame: int
    world: dict[str, Mat4]
    """Role or landmark to that joint's world transform, Blender Z-up."""


class Span(Frozen):
    """What one reading did over a whole clip."""

    lowest: float
    highest: float
    mean: float


def readings(posed: Posed, child_axis: Vec3) -> dict[str, float | None]:
    """Every reading of one frame, by label, in the order `READINGS` lists.

    `None` where the rig has no joint for a reading, and where two joints sit
    on top of each other: reporting 0 there would be a precise wrong number.
    """
    joint = posed.world.get
    return {
        "skull pitch": pitch(joint("head"), joint(SKULL_TOP)),
        "neck pitch": pitch(joint("neck"), joint("head")),
        "spine lean": pitch(joint("hips"), joint("neck")),
        "shoulder roll": roll(joint("right_arm"), joint("left_arm")),
        "left upper arm": from_down(joint("left_arm"), joint("left_forearm")),
        "right upper arm": from_down(joint("right_arm"), joint("right_forearm")),
        "left elbow bend": bend(
            joint("left_arm"), joint("left_forearm"), joint("left_hand")
        ),
        "right elbow bend": bend(
            joint("right_arm"), joint("right_forearm"), joint("right_hand")
        ),
        "left wrist bend": wrist(joint("left_forearm"), joint("left_hand"), child_axis),
        "right wrist bend": wrist(
            joint("right_forearm"), joint("right_hand"), child_axis
        ),
        "hips height": height(joint("hips")),
    }


def step(head: Mat4 | None, tail: Mat4 | None) -> Vec3 | None:
    """The direction from one joint to another, or None when the rig has no
    such joint."""
    if head is None or tail is None:
        return None
    return joint_direction(head, tail)


def pitch(head: Mat4 | None, tail: Mat4 | None) -> float | None:
    """How far the line between two joints leans off straight up.

    Positive leans forward, which is minus Y in Blender.
    """
    if (line := step(head, tail)) is None:
        return None
    if (off := degrees_between(UP, line)) is None:
        return None
    return off if dot(line, FORWARD) >= 0.0 else -off


def roll(right: Mat4 | None, left: Mat4 | None) -> float | None:
    """How far the line between the two arm joints tips off level.

    Positive lifts his left side. Read against straight up and taken off a
    quarter turn, because level is a plane and up is the one direction this
    module already states a sign for.
    """
    if (line := step(right, left)) is None:
        return None
    off = degrees_between(UP, line)
    return None if off is None else 90.0 - off


def from_down(head: Mat4 | None, tail: Mat4 | None) -> float | None:
    """How far a segment points off straight down. A hanging arm reads 0."""
    line = step(head, tail)
    return None if line is None else degrees_between(DOWN, line)


def bend(head: Mat4 | None, middle: Mat4 | None, tail: Mat4 | None) -> float | None:
    """The angle between two segments that meet. A straight limb reads 0."""
    upper, lower = step(head, middle), step(middle, tail)
    if upper is None or lower is None:
        return None
    return degrees_between(upper, lower)


def wrist(forearm: Mat4 | None, hand: Mat4 | None, child_axis: Vec3) -> float | None:
    """How far the hand bone's own axis sits off the forearm above it.

    The bone's own axis, and it is the only reading here that reads one: a
    hand is the last joint of the chain, so there is no joint below it to
    point at.
    """
    line = step(forearm, hand)
    if line is None or hand is None:
        return None
    return degrees_between(line, mat_direction(hand, child_axis))


def height(joint: Mat4 | None) -> float | None:
    """How high one joint sits, in meters. Context, not an angle."""
    return None if joint is None else mat_translation(joint)[2]


def span(values: Sequence[float | None]) -> Span | None:
    """What one reading did over a clip, or None when nothing answered."""
    read = [value for value in values if value is not None]
    if not read:
        return None
    return Span(lowest=min(read), highest=max(read), mean=sum(read) / len(read))


def chosen_frames(available: range, asked: str | None) -> tuple[int, ...]:
    """Which frames to read: the ones asked for, or every frame of the clip.

    A frame the clip does not have is refused rather than clamped, because a
    reading taken at a frame nobody animated is a number with nothing behind
    it.
    """
    if not available:
        raise ValueError("a clip with no frame has no posture to read")
    if asked is None:
        return tuple(available)
    wanted = []
    for part in asked.split(","):
        try:
            frame = int(part)
        except ValueError:
            raise ValueError(f"--frames takes whole numbers, got {part!r}") from None
        if frame not in available:
            raise ValueError(
                f"frame {frame} is not in this clip, "
                f"{available.start} to {available[-1]}"
            )
        wanted.append(frame)
    return tuple(wanted)


def report(title: str, posed: Sequence[Posed], child_axis: Vec3) -> str:
    """One reading of one clip, as the text `cargo art posture` prints.

    Two blocks: a row per frame under short column names, then a line per
    reading with its full name and its min, max and mean. The second block is
    what says which column is which.
    """
    rows = [readings(frame, child_axis) for frame in posed]
    grid = ["frame" + "".join(f"{r.column:>{COLUMN}}" for r in READINGS)]
    for frame, row in zip(posed, rows, strict=True):
        grid.append(
            f"{frame.frame:5d}"
            + "".join(cell(row[r.label], r.decimals, COLUMN) for r in READINGS)
        )

    head = ("min", "max", "mean")
    summary = [
        f"{'column':<{NAME}}{'reading':<{LABEL}}"
        + "".join(f"{name:>{COLUMN}}" for name in head)
    ]
    for reading in READINGS:
        summary.append(summarized(reading, span([row[reading.label] for row in rows])))
    return "\n".join([title, UNITS, "", *grid, "", *summary, ""])


def summarized(reading: Reading, over: Span | None) -> str:
    """One reading's name beside what it did over the whole clip."""
    read = (
        (None, None, None) if over is None else (over.lowest, over.highest, over.mean)
    )
    return f"{reading.column:<{NAME}}{reading.label:<{LABEL}}" + "".join(
        cell(value, reading.decimals, COLUMN) for value in read
    )


def cell(value: float | None, decimals: int, width: int) -> str:
    """One number as it prints, or `n/a` where the rig answered nothing."""
    if value is None:
        return f"{'n/a':>{width}}"
    return f"{value:>{width}.{decimals}f}"
