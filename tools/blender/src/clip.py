"""What the retarget reports about the clip it just wrote.

Requirement 9, as two counts per bone: channels that are not straight lines
between the frames the transfer wrote, and keys outside the frames the source
had. Both are the retarget's own invariants, so both are structurally true
when the code is right and worth nothing unless something can see them break.

Reading an F-curve needs `bpy`. Counting does not, so the counting is here
and `retarget_animation.py` only hands over what it read. The Rust side owns
the two rules themselves, in `crates/xtask-art/src/check/clip.rs`, which is
what `cargo art check --list-rules` prints.

Free of `bpy`, so it is unit tested with no Blender.
"""

from collections.abc import Iterable

from framing import Frozen, bone_from_data_path

LINEAR = "LINEAR"
"""The only interpolation the retarget writes. `keyframe_insert` writes
Bezier, which rounds off every joint's path between two sampled frames."""

CONSTANT = "CONSTANT"
"""The only extrapolation it writes: outside the clip the pose is held."""


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
