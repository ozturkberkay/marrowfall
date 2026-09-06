"""Where a foot touches the ground, and what to do about it.

Three jobs, all free of `bpy`.

- **Contact.** A foot is on the ground when its sole is low and slow. Both
  thresholds are published against a 180 cm reference and scaled to the rig
  in front of us, and the speed one is meters per second rather than meters
  per frame, so a clip means the same thing at 8 fps and at 30. A majority
  vote over an odd window then fills a one frame gap and drops a one frame
  spike, and what is left is the plant runs.
- **The lock.** Inside a run the foot is held where its first frame put it,
  horizontally, with two frame ramps either side so the correction arrives
  and leaves without a step.
- **The leg.** A two bone analytic solve moves the knee so the ankle reaches
  where the lock puts it, keeping both bone lengths and the foot's own world
  orientation, and hands back the three local rotations the action stores.
  A target the leg cannot reach is reported as the gap it fell short by,
  never as a NaN.

**The sole, not the toe joint.** Every point this module is handed is a sole
point and never a joint. Corrections 1 and 2 of T9 in the design have the
measurements that say why, and how the point is derived on a clip with no
mesh.

Free of `bpy`, so it is unit tested with no Blender. The Rust side owns the
rules themselves, in `crates/xtask-art/src/check/foot.rs`, which is what
`cargo art check --list-rules` prints.
"""

import math
from collections.abc import Sequence

from findings import Comparison, Finding, Rule
from framing import Frozen, Vec3
from transfer import (
    EPSILON,
    Quat,
    aim_rotation,
    dot,
    length,
    normalized,
    quat_inverted,
    quat_multiply,
    scaled,
    square_to,
)

REFERENCE_HEIGHT_M = 1.80
"""The height every threshold below is published against."""

CONTACT_HEIGHT_M = 0.03
"""How near the ground a sole is before it counts as touching it."""

CONTACT_SPEED_MPS = 0.30
"""And how slowly it must be moving, as a rate, so the rule means the same
thing at every clip rate."""

VOTE_FRAMES_AT_60 = 5
"""How wide the majority window is at 60 fps. Scaled to the clip's own rate,
then forced odd and at least 3: an even window carries no majority."""

RAMP_FRAMES = 2
"""How many frames the lock fades in and out over, either side of a run."""

CONTACT = (
    "each foot's sole point under the toe, at every frame of the clip, in "
    "Blender Z-up world space, against the ground plane at zero"
)

STANCE = (
    "each foot's sole point under the toe, inside one plant run, against "
    "where the run's first frame put it, horizontally, in Blender Z-up world "
    "space"
)

SOLE = (
    "the two sole points of each foot, under the ankle and under the toe, at "
    "every frame, against the ground plane at zero, in Blender Z-up world space"
)

UNDER_THE_TOE = "under the toe"
UNDER_THE_ANKLE = "under the ankle"
"""What a message calls each of the two sole points. `check/foot.rs` spells
them the same, so one reading names one place whichever side took it."""

PLANTS = Rule(
    id="clip.foot_contact.plants",
    comparison=Comparison.GE,
    unit="plant runs",
    measured_on=CONTACT,
)

SKATE = Rule(
    id="clip.foot_contact.skate",
    comparison=Comparison.LE,
    unit="meters",
    measured_on=STANCE,
)

PENETRATION = Rule(
    id="clip.foot_contact.penetration",
    comparison=Comparison.LE,
    unit="meters",
    measured_on=SOLE,
)

RULES = (PLANTS, SKATE, PENETRATION)
"""Every rule this module reports, so it asks for one published limit each."""


def scale_of(height_m: float) -> float:
    """How much bigger this rig is than the rig the thresholds are stated on."""
    if not math.isfinite(height_m) or height_m <= 0.0:
        raise ValueError(f"a rig {height_m} m tall has no height at all")
    return height_m / REFERENCE_HEIGHT_M


def vote_width(source_fps: int) -> int:
    """The majority window at this clip's own rate, odd and at least 3.

    Five frames at 60 fps, so the window covers the same slice of time
    whatever the rate. `| 1` makes it odd, because an even window can tie and
    a tie is not a majority.
    """
    return max(3, round(VOTE_FRAMES_AT_60 * source_fps / 60) | 1)


def voted(flags: Sequence[bool], width: int) -> list[bool]:
    """A majority vote over a window of `width` frames, centered on each.

    The clip is held at its ends rather than the window shrinking, so every
    frame is decided by the same number of votes.
    """
    if width < 3 or width % 2 == 0:
        raise ValueError(f"a majority window is odd and at least 3, got {width}")
    last = len(flags) - 1
    half = width // 2
    return [
        sum(flags[min(last, max(0, at + step))] for step in range(-half, half + 1)) * 2
        > width
        for at in range(len(flags))
    ]


def touching(path: Sequence[Vec3], source_fps: int, scale: float) -> list[bool]:
    """Which frames have this sole low enough and slow enough to be on it.

    The speed is the step to the neighboring frame times the clip's own rate,
    so it is meters per second. The first frame has no previous one and is
    read against the next: read against itself it would be still by
    construction, and a foot that starts low and leaves at once would plant
    for exactly one frame.
    """
    ceiling, limit = CONTACT_HEIGHT_M * scale, CONTACT_SPEED_MPS * scale
    last = len(path) - 1
    return [
        point[2] < ceiling
        and math.dist(point[:2], path[at - 1 if at else min(1, last)][:2]) * source_fps
        < limit
        for at, point in enumerate(path)
    ]


def runs_of(flags: Sequence[bool]) -> list[tuple[int, int]]:
    """Every stretch of consecutive contact frames, as index pairs."""
    found: list[tuple[int, int]] = []
    for at, flag in enumerate(flags):
        if not flag:
            continue
        if found and found[-1][1] == at - 1:
            found[-1] = (found[-1][0], at)
        else:
            found.append((at, at))
    return found


def plant_runs(
    path: Sequence[Vec3], source_fps: int, scale: float
) -> list[tuple[int, int]]:
    """The frames this foot is planted for, contact and vote together."""
    return runs_of(voted(touching(path, source_fps, scale), vote_width(source_fps)))


def locked(
    path: Sequence[Vec3], runs: Sequence[tuple[int, int]], ramp: int = RAMP_FRAMES
) -> list[Vec3]:
    """Where each frame's sole point belongs once every run is held still.

    Inside a run the point is held at the run's first frame. Either side of
    it the correction fades over `ramp` frames, so the foot arrives and
    leaves without a step. A frame two runs both reach takes the stronger of
    the two, which is the nearer run.
    """
    held = list(path)
    for at, point in enumerate(path):
        weight, target = _strongest(path, runs, at, ramp)
        if weight == 0.0:
            continue
        held[at] = (
            point[0] + (target[0] - point[0]) * weight,
            point[1] + (target[1] - point[1]) * weight,
            point[2],
        )
    return held


def _strongest(
    path: Sequence[Vec3], runs: Sequence[tuple[int, int]], at: int, ramp: int
) -> tuple[float, Vec3]:
    """The nearest run's pull on one frame, and where it pulls the foot to."""
    best, target = 0.0, path[at]
    for start, end in runs:
        away = max(start - at, at - end, 0)
        if away > ramp:
            continue
        weight = (ramp + 1 - away) / (ramp + 1)
        if weight > best:
            best, target = weight, path[start]
    return best, target


def drift(path: Sequence[Vec3], run: tuple[int, int]) -> float:
    """How far the sole wanders from where the run's first frame put it."""
    start, end = run
    return max(math.dist(path[at][:2], path[start][:2]) for at in range(start, end + 1))


class Leg(Frozen):
    """One leg at one instant, in Blender Z-up world space.

    Four world rotations, root first: the bone above the upper leg, the upper
    leg, the lower leg and the foot. `rest` is where the rig rests them and
    `pose` is where this frame put them. `joints` is where the pose put the
    hip, the knee and the ankle.
    """

    rest: tuple[Quat, Quat, Quat, Quat]
    pose: tuple[Quat, Quat, Quat, Quat]
    joints: tuple[Vec3, Vec3, Vec3]


class Reach(Frozen):
    """Where the solve put the knee and the ankle, and what it could not do."""

    knee: Vec3
    ankle: Vec3
    shortfall: float
    """Meters between the ankle asked for and the ankle reached. A leg is a
    triangle, so a target too far out or folded too tight is reported here
    rather than as a NaN two functions away."""


def bend(leg: Leg, move: Vec3) -> Reach:
    """A two bone analytic solve: move the ankle by `move`, keep both lengths.

    The knee stays in the plane the pose already bends it through, so the leg
    keeps the side it bends towards.
    """
    hip, knee, ankle = leg.joints
    target = (ankle[0] + move[0], ankle[1] + move[1], ankle[2] + move[2])
    thigh, shin = math.dist(hip, knee), math.dist(knee, ankle)
    want = (target[0] - hip[0], target[1] - hip[1], target[2] - hip[2])
    if (span := length(want)) < EPSILON:
        # The target is the hip itself, which no direction points at.
        want, span = (0.0, 0.0, -1.0), 1.0
    # A triangle, strictly: at either end the knee angle stops existing.
    reachable = min(max(span, abs(thigh - shin) + EPSILON), thigh + shin - EPSILON)
    towards = scaled(want, 1.0 / span)
    reached = (
        hip[0] + towards[0] * reachable,
        hip[1] + towards[1] * reachable,
        hip[2] + towards[2] * reachable,
    )
    return Reach(
        knee=_knee(hip, knee, ankle, towards, thigh, shin, reachable),
        ankle=reached,
        shortfall=math.dist(target, reached),
    )


def _knee(
    hip: Vec3,
    knee: Vec3,
    ankle: Vec3,
    towards: Vec3,
    thigh: float,
    shin: float,
    span: float,
) -> Vec3:
    """Where the knee goes once the ankle is `span` away, along `towards`.

    The law of cosines gives the angle at the hip, and the plane the pose
    already bends through gives the direction to swing it in. A leg with no
    bend has no such plane, and a straight one is the answer there anyway.
    """
    above = (knee[0] - hip[0], knee[1] - hip[1], knee[2] - hip[2])
    angle = math.acos(
        min(1.0, max(-1.0, (thigh**2 + span**2 - shin**2) / (2.0 * thigh * span)))
    )
    sideways = _sideways(above, towards)
    return (
        hip[0] + (towards[0] * math.cos(angle) + sideways[0] * math.sin(angle)) * thigh,
        hip[1] + (towards[1] * math.cos(angle) + sideways[1] * math.sin(angle)) * thigh,
        hip[2] + (towards[2] * math.cos(angle) + sideways[2] * math.sin(angle)) * thigh,
    )


def _sideways(above: Vec3, towards: Vec3) -> Vec3:
    """Which way the knee leans out of the hip-to-ankle line, as a unit.

    The part of the knee it already had, square to the new line, so the leg
    keeps bending the way it was bending. A leg already straight along that
    line leans nowhere and takes any square direction.
    """
    out = _apart(scaled(towards, dot(above, towards)), above)
    return square_to(towards) if length(out) < EPSILON else normalized(out)


def keys(leg: Leg, reach: Reach) -> tuple[Quat, Quat, Quat]:
    """The solve as the three local rotations the action stores.

    Each bone keeps its own roll, because the world change is the shortest
    rotation from where it pointed to where it now points. The foot's world
    orientation is held, so the toe travels with the ankle rather than
    swinging, and its own key takes back what the two bones above it turned.
    """
    hip, knee, ankle = leg.joints
    world = [
        leg.pose[0],
        quat_multiply(
            aim_rotation(_apart(hip, knee), _apart(hip, reach.knee)), leg.pose[1]
        ),
        quat_multiply(
            aim_rotation(_apart(knee, ankle), _apart(reach.knee, reach.ankle)),
            leg.pose[2],
        ),
        leg.pose[3],
    ]
    return (
        _basis(leg.rest, world, 1),
        _basis(leg.rest, world, 2),
        _basis(leg.rest, world, 3),
    )


def _apart(from_point: Vec3, to_point: Vec3) -> Vec3:
    return (
        to_point[0] - from_point[0],
        to_point[1] - from_point[1],
        to_point[2] - from_point[2],
    )


def _basis(rest: Sequence[Quat], world: Sequence[Quat], at: int) -> Quat:
    """One bone's local rotation, from its own world one and its parent's."""
    rest_relative = quat_multiply(quat_inverted(rest[at - 1]), rest[at])
    return quat_multiply(
        quat_inverted(quat_multiply(world[at - 1], rest_relative)), world[at]
    )


def plants(
    foot: str, runs: Sequence[tuple[int, int]], travels: bool, rule: Rule
) -> Finding:
    """`clip.foot_contact.plants`: how often this foot comes to rest.

    A clip the library declares in place has a ground that moves under it, so
    its feet must slide and there is no plant to count.
    """
    if not travels:
        return rule.skipped(
            foot,
            f"the library declares travels: false, so {foot} stands on ground "
            f"that moves under it and cannot plant",
        )
    if not runs:
        return rule.measured(
            foot,
            0.0,
            f"{foot} never comes to rest on the ground over the clip",
        )
    where = ", ".join(f"{start}..{end}" for start, end in runs)
    return rule.measured(
        foot,
        float(len(runs)),
        f"{foot} plants {len(runs)} time(s) over the clip, on frames {where}",
    )


def skate(
    foot: str,
    runs: Sequence[tuple[int, int]],
    drifts: Sequence[float],
    travels: bool,
    rule: Rule,
) -> list[Finding]:
    """`clip.foot_contact.skate`: how far a planted foot slides, per run.

    An empty set has no maximum worth trusting, so a foot that never plants
    is a measurement that does not exist rather than a drift of zero.
    """
    if not travels:
        return [
            rule.skipped(
                foot,
                f"the library declares travels: false, so {foot} stands on "
                f"ground that moves under it and cannot plant",
            )
        ]
    if not runs:
        return [
            rule.undefined(
                foot, f"{foot} never plants, so it has no stance to be read across"
            )
        ]
    return [
        rule.measured(
            f"{foot} run {index + 1}",
            slid,
            f"{foot} drifts {slid:.4f} m over frames {start}..{end}, where it "
            f"is planted",
        )
        for index, ((start, end), slid) in enumerate(zip(runs, drifts, strict=True))
    ]


def penetration(
    foot: str,
    ball: Sequence[float],
    heel: Sequence[float],
    frames: Sequence[int],
    rule: Rule,
) -> Finding:
    """`clip.foot_contact.penetration`: how far the sole gets under the floor.

    Both points, so the message names the one that sank rather than a depth
    with no place. Negative when neither reaches the floor at all, which is
    the clearance the foot kept. The ground plane is zero, where the rig's
    own rest pose stands.
    """
    read = [
        (height, frame, point)
        for frame, toe, ankle in zip(frames, ball, heel, strict=True)
        for height, point in ((toe, UNDER_THE_TOE), (ankle, UNDER_THE_ANKLE))
    ]
    if not read:
        return rule.undefined(
            foot, f"{foot} has no frame at all, so its sole has no height to read"
        )
    low, frame, point = min(read, key=lambda entry: entry[0])
    return rule.measured(
        foot,
        -low,
        f"the sole of {foot} {point} "
        + (
            f"sinks {-low:.4f} m below the ground at frame {frame}"
            if low < 0.0
            else f"gets to {low:.4f} m over the ground at frame {frame}"
        ),
    )
