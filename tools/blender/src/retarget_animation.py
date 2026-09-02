"""Fit a clip authored on someone else's rig onto this project's canonical one.

Blender glue only. Every decision is `transfer.py`'s, which imports no `bpy`
and is unit tested without one. This module imports the two rigs, reads world
matrices, hands them over, and writes the local poses back as keys.

A bought clip cannot be baked as it arrives, for three reasons:

  - **Bone names.** Providers namespace them (`mixamorig:Hips`) and disagree
    about which bone a name means, so bones are paired by anatomical role out
    of `art/skeletons/<skeleton>.toml`, never by name.
  - **Extra bones.** A provider's skeleton has fingers. This project's
    humanoid is 24 bones with none, so those curves have nowhere to go.
  - **Rest pose.** Every rig rolls its bones its own way, and our legs sit 174
    degrees of pure roll from Mixamo's. Motion is therefore transferred in
    world space against the absolute aim table, and the difference between the
    two rest poses is measured once, per role, as a constant offset.

Two things are deliberate here. **No object transform is ever applied**, only
composed: applying one to a rig that owns an action rescales the rest and
leaves every location key byte identical, so 2.316 m of travel silently
becomes 231.599 m. And **nothing is ever read back out of the target rig**:
its local poses are computed algebraically, so the dependency graph is
evaluated once per source frame rather than once per bone per frame.

    blender --background --python-use-system-env \
        --python tools/blender/src/retarget_animation.py -- \
        --source art/staging/downloads/walk_back.fbx \
        --rig art/skeletons/humanoid.glb --convention mixamo \
        --out art/animations/local/walk_back.glb --name walk_back
"""

import argparse
import pathlib
import sys

import bpy
from bake_sprites import action_fcurves, assign_action, scale_translation
from clip import CONSTANT, LINEAR, Channel, defects
from findings import Comparison, Finding, Severity, attempt, guard, write_report
from framing import translation_scale
from mathutils import Matrix
from skeleton import Skeleton, bare_bone_name, unfilled_roles
from strip_animation import skin_carrier
from transfer import (
    Bone,
    LocalPose,
    Mat4,
    offsets,
    quat_degrees,
    reference_pose,
    segment_length,
    transfer,
)

FRAME_TOLERANCE = 1e-4
"""How far a key time may sit off a whole frame. T7's `clip.fps_grid` limit."""

INTERPOLATION = "clip.interpolation"
REFERENCE_POSE_KEY = "clip.reference_pose_key"
CHANNELS = "the F-curves of the output action, before export"


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--source",
        type=pathlib.Path,
        required=True,
        help="The downloaded clip, .fbx or .glb, chosen by extension.",
    )
    parser.add_argument(
        "--rig",
        type=pathlib.Path,
        required=True,
        help="The canonical rig every clip for this skeleton is fitted to.",
    )
    parser.add_argument(
        "--convention",
        required=True,
        help="How the source names its bones, a table in the rig's role map.",
    )
    parser.add_argument("--out", type=pathlib.Path, required=True)
    parser.add_argument(
        "--name", required=True, help="Library name, which the action is stored under."
    )
    return parser.parse_args(argv)


def read_skeleton(rig: pathlib.Path) -> Skeleton:
    """The skeleton file beside the rig, `humanoid.glb` -> `humanoid.toml`.

    Derived rather than passed, so the pair cannot be mismatched.
    """
    path = rig.with_suffix(".toml")
    if not path.exists():
        sys.exit(f"error: {path} not found, so nothing can be matched to {rig.name}")
    return Skeleton.parse(path.read_text())


def import_armature(path: pathlib.Path) -> bpy.types.Object:
    """Imports one file and returns the armature it brought, by extension.

    The armature this import added, not the first one in the scene: the
    canonical rig is already loaded by the time the clip arrives.
    """
    if not path.exists():
        sys.exit(f"error: {path} not found")
    known = set(bpy.data.objects)
    if path.suffix.lower() == ".glb":
        bpy.ops.import_scene.gltf(filepath=str(path))
    elif path.suffix.lower() == ".fbx":
        bpy.ops.import_scene.fbx(filepath=str(path))
    else:
        sys.exit(f"error: {path} is neither .fbx nor .glb")

    armature = next(
        (o for o in bpy.data.objects if o.type == "ARMATURE" and o not in known), None
    )
    if armature is None:
        sys.exit(f"error: {path} has no armature, so there is nothing to retarget")
    return armature


def keep_only(armature: bpy.types.Object) -> None:
    """Empties the scene of everything but one armature.

    Both files arrive with a body attached: the rig with its skin carrier, a
    provider's clip with a whole stock character. Only the canonical skeleton
    is exported, so the rest goes.
    """
    for obj in list(bpy.data.objects):
        if obj is not armature:
            bpy.data.objects.remove(obj, do_unlink=True)
    for image in list(bpy.data.images):
        bpy.data.images.remove(image)
    for material in list(bpy.data.materials):
        bpy.data.materials.remove(material)


def source_action(known: set[bpy.types.Action], path: pathlib.Path) -> bpy.types.Action:
    """The one action the source file brought, which says which frames exist."""
    fresh = [action for action in bpy.data.actions if action not in known]
    if not fresh:
        sys.exit(f"error: {path.name} contains no animation")
    if len(fresh) > 1:
        names = [action.name for action in fresh]
        sys.exit(f"error: {path.name} holds {len(names)} actions, expected 1: {names}")
    return fresh[0]


def source_frames(action: bpy.types.Action, path: pathlib.Path) -> range:
    """The frames the source really has, refused if they are not whole.

    glTF stores key times in seconds, so the scene's rate decides which
    frames they land on: a 30 fps clip read in a 24 fps scene spans 0.8 to
    16.8, and rounding that to 1 to 17 drops four frames without a word. T7
    sets the scene rate from the library's `source_fps` and reports
    `clip.fps_grid`; until then a range that is not whole is refused.
    """
    lo, hi = action.frame_range
    for edge in (lo, hi):
        if abs(edge - round(edge)) > FRAME_TOLERANCE:
            sys.exit(
                f"error: {path.name} spans frames {lo} to {hi} in a "
                f"{bpy.context.scene.render.fps} fps scene, which is not a whole "
                f"frame range. Its own rate is not the scene's, and sampling it "
                f"on this grid would drop frames"
            )
    return range(round(lo), round(hi) + 1)


def as_mat4(matrix: Matrix) -> Mat4:
    """A Blender matrix as the plain rows `transfer` works in."""
    rows = [(row[0], row[1], row[2], row[3]) for row in matrix]
    return (rows[0], rows[1], rows[2], rows[3])


def bones_by_role(
    armature: bpy.types.Object, convention: dict[str, str]
) -> dict[str, str]:
    """Role to the bone that fills it on this armature, by bare name.

    Two bones that reduce to the same name refuse rather than one of them
    winning: the transfer would drive whichever it kept and freeze the other,
    and nothing downstream could tell which.
    """
    role_of = {bare_bone_name(bone): role for role, bone in convention.items()}
    filled: dict[str, str] = {}
    for bone in armature.data.bones:
        role = role_of.get(bare_bone_name(bone.name))
        if role is None:
            continue
        if (taken := filled.get(role)) is not None:
            sys.exit(
                f"error: {armature.name} has two bones for the {role!r} role, "
                f"{taken} and {bone.name}, which are the same name once the "
                f"namespace and the case are stripped"
            )
        filled[role] = bone.name
    return filled


def refuse_unfilled(
    path: pathlib.Path, armature: bpy.types.Object, skeleton: Skeleton, convention: str
) -> dict[str, str]:
    """The roles this file fills, or an exit naming the ones it does not.

    A role nothing drives is refused rather than skipped: the code this
    replaces left seven bones uncorrected and reported success.
    """
    names = [bone.name for bone in armature.data.bones]
    if absent := unfilled_roles(skeleton.convention(convention), names):
        wanted = sorted(set(absent) - set(skeleton.optional_roles))
        if wanted:
            sys.exit(
                f"error: {path.name} has no bone for {len(wanted)} role(s) under "
                f"the {convention!r} naming convention, {wanted[:6]}, so those "
                f"would go undriven. Its bones are named like {sorted(names)[:4]}"
            )
    return bones_by_role(armature, skeleton.convention(convention))


def rest_in_world(
    armature: bpy.types.Object, by_role: dict[str, str]
) -> dict[str, Mat4]:
    """Each role's rest transform in world space.

    `matrix_world @ bone.matrix_local`, composed and never applied.
    """
    return {
        role: as_mat4(armature.matrix_world @ armature.data.bones[bone].matrix_local)
        for role, bone in by_role.items()
    }


def pose_in_world(
    armature: bpy.types.Object, by_role: dict[str, str]
) -> dict[str, Mat4]:
    """Each role's world transform at the scene's current frame."""
    return {
        role: as_mat4(armature.matrix_world @ armature.pose.bones[bone].matrix)
        for role, bone in by_role.items()
    }


def target_bones(
    armature: bpy.types.Object, by_role: dict[str, str], driven: set[str]
) -> tuple[Bone, ...]:
    """Our rig as `transfer` takes it: the whole hierarchy, roles attached.

    Bones outside `driven` carry no role, so the transfer holds them at rest
    and their driven children still compose from the right place.
    """
    role_of = {bone: role for role, bone in by_role.items() if role in driven}
    return tuple(
        Bone(
            name=bone.name,
            parent=None if bone.parent is None else bone.parent.name,
            role=role_of.get(bone.name),
            rest=as_mat4(bone.matrix_local),
        )
        for bone in armature.data.bones
    )


def write_keys(
    armature: bpy.types.Object, name: str, poses: dict[int, dict[str, LocalPose]]
) -> bpy.types.Action:
    """Every frame's local poses, as one action on `armature`.

    Rotation only for every bone but the root. A non-root bone keyed with a
    location would drag our joint onto the source's, and our limb lengths
    would follow.
    """
    action = bpy.data.actions.new(name)
    assign_action(armature, action)
    for posed in armature.pose.bones:
        # The rig arrives carrying its own bind-pose action, and replacing an
        # action does not undo the pose it left behind. Every bone this
        # source does not drive has to be at rest, or an undriven arm keeps
        # whatever the last action put there.
        posed.matrix_basis = Matrix()
        posed.rotation_mode = "QUATERNION"
    for frame, bones in sorted(poses.items()):
        for bone, pose in bones.items():
            posed = armature.pose.bones[bone]
            posed.rotation_quaternion = pose.rotation
            posed.keyframe_insert("rotation_quaternion", frame=frame)
            if pose.location is not None:
                posed.location = pose.location
                posed.keyframe_insert("location", frame=frame)
    return action


def linear_and_constant(action: bpy.types.Action) -> None:
    """Straight lines between keys, and a held pose outside them.

    `keyframe_insert` writes Bezier handles, which round off every joint's
    path between two sampled frames.
    """
    for curve in action_fcurves(action):
        curve.extrapolation = "CONSTANT"
        for key in curve.keyframe_points:
            key.interpolation = "LINEAR"
        curve.update()


def channels(action: bpy.types.Action) -> list[Channel]:
    """Every F-curve of the action, as the two clip rules read it."""
    return [
        Channel(
            data_path=curve.data_path,
            interpolations=tuple(key.interpolation for key in curve.keyframe_points),
            extrapolation=curve.extrapolation,
            frames=tuple(key.co[0] for key in curve.keyframe_points),
        )
        for curve in action_fcurves(action)
    ]


def measure(action: bpy.types.Action, frames: range) -> list[Finding]:
    """What this run reports: requirement 9, per bone.

    Both rules count defects, so neither has a tunable limit. The counting
    itself is `clip.defects`, which imports no `bpy` and has its own
    negatives.
    """
    counted = defects(channels(action), frames)
    return [
        finding(
            INTERPOLATION,
            bone,
            count.not_linear,
            "channels",
            f"{bone} has {count.not_linear} channel(s) that are not {LINEAR} "
            f"with {CONSTANT} extrapolation",
        )
        for bone, count in sorted(counted.items())
    ] + [
        finding(
            REFERENCE_POSE_KEY,
            bone,
            count.outside_range,
            "keys",
            f"{bone} carries {count.outside_range} key(s) outside the "
            f"source's frame range {frames.start}..{frames.stop - 1}",
        )
        for bone, count in sorted(counted.items())
    ]


def finding(rule: str, subject: str, measured: int, unit: str, message: str) -> Finding:
    """One defect count against a limit of zero.

    The comparison decides the severity, never this caller: a rule that
    filed its own defect as information would be quiet about it.
    """
    comparison, limit = Comparison.EQ, 0.0
    return Finding(
        rule=rule,
        severity=(
            Severity.INFO if comparison.holds(measured, limit) else Severity.ERROR
        ),
        subject=subject,
        measured=float(measured),
        limit=limit,
        comparison=comparison,
        unit=unit,
        attempt=attempt(),
        measured_on=CHANNELS,
        message=message,
    )


def export(out: pathlib.Path, armature: bpy.types.Object) -> None:
    """Writes the canonical armature and its new action, with a skin carrier.

    glTF has no standalone armature: bones only survive as part of a skin, so
    a one-triangle mesh is what carries this file's skeleton through.
    """
    carrier = skin_carrier(armature)
    out.parent.mkdir(parents=True, exist_ok=True)

    bpy.ops.object.select_all(action="DESELECT")
    armature.select_set(True)
    carrier.select_set(True)
    bpy.context.view_layer.objects.active = armature
    bpy.ops.export_scene.gltf(
        filepath=str(out),
        export_format="GLB",
        use_selection=True,
        export_animations=True,
        export_skins=True,
        export_materials="NONE",
    )


def retarget(
    source_path: pathlib.Path,
    rig_path: pathlib.Path,
    out: pathlib.Path,
    name: str,
    convention: str,
) -> None:
    bpy.ops.wm.read_factory_settings(use_empty=True)
    skeleton = read_skeleton(rig_path)
    ours = import_armature(rig_path)
    keep_only(ours)
    our_roles = refuse_unfilled(rig_path, ours, skeleton, skeleton.canonical)

    known = set(bpy.data.actions)
    source = import_armature(source_path)
    action = source_action(known, source_path)
    source_roles = refuse_unfilled(source_path, source, skeleton, convention)

    # Only the roles both rigs fill are driven. Anything else our rig holds at
    # rest, so its driven children still compose from the right place.
    driven = {role: source_roles[role] for role in source_roles if role in our_roles}
    if not driven:
        sys.exit(
            f"error: {source_path.name} and {rig_path.name} fill no role in "
            f"common, so there is nothing to transfer"
        )
    chain = {
        role: parent
        for role in driven
        if (parent := skeleton.chain_parent(role, driven)) is not None
    }
    aim = {role: skeleton.aim(role) for role in driven}
    rest_ours = rest_in_world(ours, our_roles)
    rest_source = rest_in_world(source, source_roles)
    offset = offsets(
        reference_pose({r: rest_ours[r] for r in driven}, chain, aim),
        reference_pose({r: rest_source[r] for r in driven}, chain, aim),
    )

    bones = target_bones(ours, our_roles, set(driven))
    object_matrix = as_mat4(ours.matrix_world)
    frames = source_frames(action, source_path)
    poses = {}
    for frame in frames:
        bpy.context.scene.frame_set(frame)
        poses[frame] = transfer(
            bones, object_matrix, pose_in_world(source, driven), offset
        )

    stride = skeleton.stride_segment
    ratio = translation_scale(
        segment_length(rest_source, *stride), segment_length(rest_ours, *stride)
    )
    keep_only(ours)
    # The source's own action outlives its armature, and the exporter would
    # write it into the GLB beside ours.
    bpy.data.actions.remove(action)
    fitted = write_keys(ours, name, poses)
    scale_translation(fitted, ratio)
    linear_and_constant(fitted)
    write_report(measure(fitted, frames))
    export(out, ours)

    worst = max(driven, key=lambda role: quat_degrees(offset[role]))
    print(
        f"retargeted {name}: {len(driven)} role(s) onto {rig_path.name} over "
        f"{len(frames)} frame(s), worst offset {worst} "
        f"{quat_degrees(offset[worst]):.2f} deg, root travel sized by "
        f"{ratio:.4f} -> {out}"
    )


def main() -> None:
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    args = parse_args(argv)
    retarget(args.source, args.rig, args.out, args.name, args.convention)


def save_blend(path: pathlib.Path) -> None:
    """The scene as it stood when the script failed, for a human to open."""
    bpy.ops.wm.save_as_mainfile(filepath=str(path), copy=True)


if __name__ == "__main__":
    # Blender exits 0 on an uncaught exception, so a failed fit would look like
    # success and leave a clip nobody can bake. `guard` writes the success
    # sentinel the Rust side asserts.
    guard(main, save_blend)
