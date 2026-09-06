"""What the retarget reports about the clip it just wrote.

Two jobs, both free of `bpy`.

- **The counts.** Requirement 9, per bone: channels that are not straight
  lines between the frames the transfer wrote, and keys outside the frames the
  source had. Both are the transfer's own invariants, so both are structurally
  true when the code is right and worth nothing unless something can see them
  break.
- **The source motion sidecar.** `clip.swing` and `clip.twist` measure the
  delivered GLB against the vendor file, and the vendor file is an FBX that
  the Rust `gltf` reader cannot open. So this run writes the source's own
  world orientations beside the report, and Rust reads the GLB and the sidecar
  together.

Reading an F-curve or a pose needs `bpy`. Counting and recording do not, so
they are here and `retarget_animation.py` only hands over what it read. The
Rust side owns the rules themselves, in
`crates/xtask-art/src/check/clip.rs`, which is what
`cargo art check --list-rules` prints.

Free of `bpy`, so it is unit tested with no Blender.
"""

import pathlib
from collections.abc import Iterable

from framing import Frozen, bone_from_data_path
from pydantic import model_validator
from transfer import Mat4, Quat, mat_rotation

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


class Frame(Frozen):
    """Where the source's bones pointed at one instant of the clip."""

    seconds: float
    """Time from the clip's own first frame. The output GLB stores key times
    in seconds too, so the two align on a quantity neither side indexes."""
    rotations: dict[str, Quat]
    """Role to that bone's world rotation, in Blender Z-up world space."""


class SourceMotion(Frozen):
    """The vendor clip's own world orientations, as the gate reads them.

    `clip.swing` and `clip.twist` are absolute rules: they measure the
    delivered GLB against the file the motion was bought in. That file is an
    FBX, the Rust `gltf` reader cannot open one, and CI has no Blender. This
    record is the bridge. Blender writes it here, beside the report, and
    `crates/xtask-art/src/check/clip.rs` reads it with the output GLB.

    Rotations only. Both rules read where a bone points and how far it is
    rolled about its own length, and neither reads a position.
    """

    rest: dict[str, Quat]
    """Role to that bone's rest world rotation. `clip.twist` measures each rig
    against its own rest, which is what makes the 174 degrees of convention
    difference cancel instead of failing every correct clip."""
    frames: tuple[Frame, ...]

    @model_validator(mode="after")
    def every_frame_must_carry_every_role_the_rest_pose_has(self) -> "SourceMotion":
        """A role missing from one frame would leave that frame unmeasured,
        and a rule with a hole in it is the failure this pipeline shipped."""
        if not self.frames:
            raise ValueError("a source motion with no frame measures nothing")
        for frame in self.frames:
            if set(frame.rotations) != set(self.rest):
                odd = sorted(set(frame.rotations) ^ set(self.rest))
                raise ValueError(
                    f"the frame at {frame.seconds} s disagrees with the rest "
                    f"pose about {odd}"
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
        rest={role: mat_rotation(matrix, role) for role, matrix in rest.items()},
        frames=tuple(
            Frame(
                seconds=(frame - first) / rate,
                rotations={
                    role: mat_rotation(matrix, role) for role, matrix in world.items()
                },
            )
            for frame, world in sorted(frames.items())
        ),
    )
