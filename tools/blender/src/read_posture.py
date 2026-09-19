"""Read what a clip did to a body, frame by frame.

Blender glue only. Every decision is `posture.py`'s, which imports no `bpy`
and is unit tested without one. This module opens the files, reads world
matrices out of them, and writes the text `cargo art posture` prints.

It measures and nothing else. No rule, no limit, no severity and no report.

Three things can be read, and the two paths say which:

- `--rig` and `--clip`: a fitted clip, played on the canonical rig. Today's
  rig and not the one the clip carries, so a clip fitted to a rig that has
  since been replaced reads as what it now is.
- `--clip` alone: a vendor file on its own rig, in the vendor's own naming.
  Blender is unavoidable here, because a Mixamo source is an FBX.
- `--rig` alone: that rig's bind pose, read off the bones themselves rather
  than off frame 0, so no action can stand in for a rest pose.

    blender --background --python-use-system-env \
        --python tools/blender/src/read_posture.py -- \
        --skeleton art/skeletons/humanoid.toml --convention standard \
        --child-axis 0,1,0 --rig art/skeletons/humanoid.glb \
        --clip art/animations/idle.glb --source-fps 24 \
        --out art/staging/reports/posture.idle.1.txt
"""

import argparse
import pathlib
import sys

import bpy
import posture
from actions import assign_action
from findings import guard
from retarget_animation import (
    bones_by_role,
    import_armature,
    pose_in_world,
    refuse_unfilled,
    rest_in_world,
    set_rate,
    source_action,
    source_frames,
)
from skeleton import Skeleton, unfilled_roles


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--skeleton",
        type=pathlib.Path,
        required=True,
        help="The skeleton file whose roles and landmarks the body is read by.",
    )
    parser.add_argument(
        "--convention", required=True, help="How the file being read names its bones."
    )
    parser.add_argument(
        "--child-axis",
        required=True,
        help=(
            "Which of a bone's own axes points at its child, as three comma "
            "separated numbers. Only the wrist reading needs it."
        ),
    )
    parser.add_argument(
        "--rig",
        type=pathlib.Path,
        help="The armature to read. Its own bind pose when there is no --clip.",
    )
    parser.add_argument(
        "--clip", type=pathlib.Path, help="The file holding the motion, .glb or .fbx."
    )
    parser.add_argument(
        "--source-fps",
        type=int,
        help="The rate the clip's own keys sit on. Required with --clip.",
    )
    parser.add_argument(
        "--frames", help="Only these frames, comma separated. Every frame by default."
    )
    parser.add_argument("--out", type=pathlib.Path, required=True)
    return parser.parse_args(argv)


def axis_from(entry: str) -> posture.Vec3:
    """`0,1,0` into the direction a bone's own axis points along."""
    try:
        x, y, z = (float(part) for part in entry.split(","))
    except ValueError:
        sys.exit(f"error: --child-axis needs three numbers, got {entry!r}")
    return (x, y, z)


def joints_of(
    path: pathlib.Path,
    armature: bpy.types.Object,
    skeleton: Skeleton,
    convention: str,
) -> dict[str, str]:
    """Every joint a reading needs, by role, with the landmarks beside them.

    A landmark the rig does not carry is refused rather than skipped: the
    head reading is what the landmark table exists for, and a missing one
    would print `n/a` forever with nothing saying why.
    """
    marks = skeleton.landmarks_of(convention)
    names = [bone.name for bone in armature.data.bones]
    if absent := unfilled_roles(marks, names):
        sys.exit(
            f"error: {path.name} has no bone for the {absent} landmark(s) of "
            f"the {convention!r} convention, so those cannot be measured. Its "
            f"bones are named like {sorted(names)[:4]}"
        )
    return refuse_unfilled(path, armature, skeleton, convention) | bones_by_role(
        armature, marks
    )


def posed_at(
    armature: bpy.types.Object, joints: dict[str, str], frames: tuple[int, ...]
) -> tuple[posture.Posed, ...]:
    """The whole body, at each frame the readings are taken on."""
    posed = []
    for frame in frames:
        bpy.context.scene.frame_set(frame)
        posed.append(posture.Posed(frame=frame, world=pose_in_world(armature, joints)))
    return tuple(posed)


def title_of(args: argparse.Namespace) -> str:
    """One line saying what was read, which the text opens with."""
    naming = f"{args.convention} naming"
    if args.clip is None:
        return f"rest pose of {args.rig.name}, {naming}"
    if args.rig is None:
        return f"{args.clip.name} on its own rig, {naming}"
    return f"{args.clip.name} on {args.rig.name}, {naming}"


def read(args: argparse.Namespace) -> None:
    if args.rig is None and args.clip is None:
        sys.exit("error: nothing to read, pass --rig or --clip or both")
    if args.clip is not None and args.source_fps is None:
        sys.exit("error: --clip needs --source-fps, the rate its own keys sit on")

    bpy.ops.wm.read_factory_settings(use_empty=True)
    skeleton = Skeleton.parse(args.skeleton.read_text())
    # Before the imports: the glTF importer turns key times in seconds into
    # frames using whatever rate the scene is on.
    if args.source_fps is not None:
        set_rate(args.source_fps)

    known = set(bpy.data.actions)
    read_from = args.rig if args.rig is not None else args.clip
    armature = import_armature(read_from)
    joints = joints_of(read_from, armature, skeleton, args.convention)

    if args.clip is None:
        posed = (posture.Posed(frame=0, world=rest_in_world(armature, joints)),)
    else:
        if args.rig is not None:
            # Only what the clip brings counts as its action, because a rig
            # can own one of its own. The armature it brings is read by
            # nothing.
            known = set(bpy.data.actions)
            import_armature(args.clip)
        # And again after: the FBX importer sets the scene from the file,
        # which would leave the rate to the vendor rather than to the library.
        set_rate(args.source_fps)
        action = source_action(known, args.clip)
        assign_action(armature, action)
        frames = posture.chosen_frames(source_frames(action), args.frames)
        posed = posed_at(armature, joints, frames)

    text = posture.report(title_of(args), posed, axis_from(args.child_axis))
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(text)
    print(text)


def main() -> None:
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    read(parse_args(argv))


def save_blend(path: pathlib.Path) -> None:
    """The scene as it stood when the script failed, for a human to open."""
    bpy.ops.wm.save_as_mainfile(filepath=str(path), copy=True)


if __name__ == "__main__":
    # Blender exits 0 on an uncaught exception, so an unread clip would look
    # like a read one. `guard` writes the success sentinel the Rust side
    # asserts.
    guard(main, save_blend)
