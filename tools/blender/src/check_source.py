"""Measure a vendor clip before anything is fitted to it.

Blender glue only. Every decision is `source.py`'s, which imports no `bpy` and
is unit tested without one. This module imports the downloaded FBX, reads
world matrices out of it, and hands them over.

It runs between the download and the retarget, on the file as it arrived, for
two reasons. A clip whose own rate or root motion is not what the library
declares is wrong before a single minute is spent fitting it. And the vendor
rig's own geometry is on record from that point on, which is the only place
`source.child_axis` can ever be read: the transfer keeps our bone names and
our rest pose, so nothing downstream still holds the vendor's.

    blender --background --python-use-system-env \
        --python tools/blender/src/check_source.py -- \
        --source art/staging/downloads/strafe_left.fbx \
        --skeleton art/skeletons/humanoid.toml --convention mixamo \
        --source-fps 30 --travels true \
        --children hips=spine_lower,neck=head --child-axis 0,1,0 \
        --limit source.traveling=0.02
"""

import argparse
import pathlib
import sys

import bpy
import source
from findings import guard, limits_from, write_report
from retarget_animation import (
    as_mat4,
    import_armature,
    refuse_unfilled,
    rest_in_world,
    source_action,
    source_grid,
)
from skeleton import Skeleton


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=pathlib.Path, required=True)
    parser.add_argument(
        "--skeleton",
        type=pathlib.Path,
        required=True,
        help="The skeleton file whose role map and tails the clip is read against.",
    )
    parser.add_argument("--convention", required=True)
    parser.add_argument(
        "--source-fps",
        type=int,
        required=True,
        help="The rate the library declares for this clip.",
    )
    parser.add_argument(
        "--travels",
        required=True,
        choices=["true", "false"],
        help="What the library declares this clip's hips do.",
    )
    parser.add_argument(
        "--children",
        required=True,
        help=(
            "ROLE=CHILD pairs, comma separated: which role each bone's own "
            "axis must point at. The Rust side reads them off `[profile.tails]`."
        ),
    )
    parser.add_argument(
        "--child-axis",
        required=True,
        help=(
            "Which of a bone's own axes points at its child, as three "
            "comma separated numbers. `[profile] child_axis` names it and the "
            "Rust side turns the letter into a direction."
        ),
    )
    parser.add_argument(
        "--limit",
        action="append",
        default=[],
        metavar="RULE=NUMBER",
        help="The published limit for one rule, passed by the runner.",
    )
    return parser.parse_args(argv)


def axis_from(entry: str) -> source.Vec3:
    """`0,1,0` into the direction a bone's own axis points along."""
    try:
        x, y, z = (float(part) for part in entry.split(","))
    except ValueError:
        sys.exit(f"error: --child-axis needs three numbers, got {entry!r}")
    return (x, y, z)


def children_from(entry: str) -> dict[str, str]:
    """`hips=spine_lower,neck=head` into the map `source.py` reads."""
    pairs = {}
    for pair in entry.split(","):
        role, _, child = pair.partition("=")
        if not role or not child:
            sys.exit(f"error: --children needs ROLE=CHILD pairs, got {pair!r}")
        pairs[role] = child
    return pairs


def hips_path(
    armature: bpy.types.Object, bone: str, frames: range
) -> tuple[source.Vec3, ...]:
    """Where the hips sat, in world space, at every frame of the clip."""
    path = []
    for frame in frames:
        bpy.context.scene.frame_set(frame)
        head = armature.matrix_world @ armature.pose.bones[bone].matrix.to_translation()
        path.append((head.x, head.y, head.z))
    return tuple(path)


def posture_at(
    armature: bpy.types.Object, by_role: dict[str, str], frames: tuple[int, ...]
) -> tuple[source.Posed, ...]:
    """The whole rig, at each frame the posture is read on."""
    posed = []
    for frame in frames:
        bpy.context.scene.frame_set(frame)
        posed.append(
            source.Posed(
                frame=frame,
                world={
                    role: as_mat4(
                        armature.matrix_world @ armature.pose.bones[bone].matrix
                    )
                    for role, bone in by_role.items()
                },
            )
        )
    return tuple(posed)


def check(args: argparse.Namespace) -> None:
    bpy.ops.wm.read_factory_settings(use_empty=True)
    skeleton = Skeleton.parse(args.skeleton.read_text())
    known = set(bpy.data.actions)
    armature = import_armature(args.source)
    action = source_action(known, args.source)
    by_role = refuse_unfilled(args.source, armature, skeleton, args.convention)

    scene = bpy.context.scene
    lo, hi = action.frame_range
    frames = range(round(lo), round(hi) + 1)
    findings = source.findings(
        source.Clip(
            declared_fps=args.source_fps,
            travels=args.travels == "true",
            # Blender's real rate is `fps / fps_base`: a 29.97 scene stores
            # 30 and 1.001.
            scene_fps=scene.render.fps / scene.render.fps_base,
            key_frames=source_grid(action).keys,
            hips=hips_path(armature, by_role[skeleton.chain_top], frames),
            axis=axis_from(args.child_axis),
            rest=rest_in_world(armature, by_role),
            children=children_from(args.children),
            posture=posture_at(armature, by_role, source.sampled_frames(frames)),
        ),
        limits_from(args.limit),
    )
    write_report(findings)
    print(
        f"measured {args.source.name}: {len(findings)} finding(s) over "
        f"{len(frames)} frame(s) at {scene.render.fps} fps"
    )


def main() -> None:
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    check(parse_args(argv))


def save_blend(path: pathlib.Path) -> None:
    """The scene as it stood when the script failed, for a human to open."""
    bpy.ops.wm.save_as_mainfile(filepath=str(path), copy=True)


if __name__ == "__main__":
    # Blender exits 0 on an uncaught exception, so an unmeasured clip would
    # look like a measured one. `guard` writes the success sentinel the Rust
    # side asserts.
    guard(main, save_blend)
