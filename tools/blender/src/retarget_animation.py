"""Fit a clip authored on someone else's rig onto this project's canonical one.

A bought clip cannot be baked as it arrives, for three reasons:

  - **Bone names.** Providers namespace them (`mixamorig:Hips`) and disagree
    about which bone a name means, so bones are paired by anatomical role out
    of `art/skeletons/<skeleton>.toml`, never by name.
  - **Extra bones.** A provider's skeleton has fingers. This project's humanoid
    is 24 bones with none, so those curves have nowhere to go.
  - **Rest pose.** Stock provider bodies are T-posed and every character here is
    A-posed. An action holds each bone's rotation *relative to its own rest*, so
    playing one on the other adds that difference to every joint, which
    `bind_pose_mismatch` refuses at 64 degrees of arm.

All three belong to the skeleton rather than to any character, so they are
fixed once, here, when the clip is fetched. Sizing the translation to the body
playing it stays with the bake: only the bake knows which body that is.

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
from bake_sprites import (
    action_fcurves,
    assign_action,
    edit_rotation_curves,
    fcurve_owners,
    rest_points,
    rotation_curves,
    scale_translation,
)
from findings import guard
from framing import (
    Vec4,
    bone_from_data_path,
    loop_mismatch,
    rest_height,
    translation_scale,
)
from mathutils import Matrix, Quaternion
from skeleton import Skeleton, bare_bone_name, unfilled_roles
from strip_animation import skin_carrier


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


def sole_action(known: set[bpy.types.Action], name: str) -> bpy.types.Action:
    """The one action the source file brought, renamed to the library name."""
    fresh = [action for action in bpy.data.actions if action not in known]
    if not fresh:
        sys.exit("error: the source file contains no animation")
    if len(fresh) > 1:
        names = [action.name for action in fresh]
        sys.exit(f"error: the source holds {len(names)} actions, expected 1: {names}")
    fresh[0].name = name
    return fresh[0]


def matched_bones(source: bpy.types.Object, roles: dict[str, str]) -> dict[str, str]:
    """Source bone name to canonical bone name, for the bones that have a role."""
    return {
        bone.name: match
        for bone in source.data.bones
        if (match := roles.get(bare_bone_name(bone.name))) is not None
    }


def drop_unmatched_curves(
    action: bpy.types.Action, matched: dict[str, str]
) -> list[str]:
    """Removes every curve the canonical rig has nowhere to put.

    Fingers, and anything else: a curve on the armature object itself would
    travel the whole character, which the bake pins only on bones.
    """
    dropped = set()
    for owner in fcurve_owners(action):
        for curve in list(owner.fcurves):
            bone = bone_from_data_path(curve.data_path)
            if bone in matched:
                continue
            dropped.add(bone or curve.data_path)
            owner.fcurves.remove(curve)
    return sorted(dropped)


def rename_to_canonical(action: bpy.types.Action, matched: dict[str, str]) -> None:
    """Repoints every remaining curve at its canonical bone.

    Runs after the unmatched curves are dropped, so every curve left has a
    counterpart to be pointed at.
    """
    for curve in action_fcurves(action):
        bone = bone_from_data_path(curve.data_path)
        _, _, property_path = curve.data_path.partition('"]')
        curve.data_path = f'pose.bones["{matched[bone]}"]{property_path}'


def rest_to_parent(bone: bpy.types.Bone) -> Matrix:
    """A bone's rest transform, measured against its parent's rest.

    The space a keyframe is in: a posed bone is its parent, then this rest
    transform, then the keyed rotation.
    """
    if bone.parent is None:
        return bone.matrix_local.copy()
    return bone.parent.matrix_local.inverted() @ bone.matrix_local


def rebase_bone(action: bpy.types.Action, bone: str, delta: Quaternion) -> None:
    """Composes one bone's rest difference onto every keyframe.

    On the parent's side of the keyed rotation, because that is the side the
    rest difference sits on: `rest_c @ keyed_c == rest_s @ keyed_s`.
    """
    edit_rotation_curves(action, bone, lambda current: delta @ current)


def rebase_rotations(
    action: bpy.types.Action,
    source: bpy.types.Object,
    canonical: bpy.types.Object,
    matched: dict[str, str],
) -> None:
    """Re-expresses every rotation against the canonical rest pose.

    One fixed correction per bone, so a T-posed source drives an A-posed rig to
    the same posed result. Identical rigs give an identity correction, which is
    what makes refitting one of our own clips a no-op.
    """
    for source_name, canonical_name in matched.items():
        delta = (
            rest_to_parent(canonical.data.bones[canonical_name]).inverted()
            @ rest_to_parent(source.data.bones[source_name])
        ).to_quaternion()
        rebase_bone(action, canonical_name, delta)


def keyed_rotation(curves: list[bpy.types.FCurve], index: int) -> Vec4:
    """One keyframe's rotation, read off the four curves that hold it."""
    w, x, y, z = (curve.keyframe_points[index].co[1] for curve in curves)
    return (w, x, y, z)


def report_loop(action: bpy.types.Action, bones: list[str]) -> None:
    """Warns when the clip does not end where it starts.

    Advisory on purpose: a clip that returns to its first pose loops cleanly,
    but whether a given motion should is a judgement call, not a rule.
    """
    first: dict[str, Vec4] = {}
    last: dict[str, Vec4] = {}
    for bone in bones:
        if curves := rotation_curves(action, bone):
            first[bone] = keyed_rotation(curves, 0)
            last[bone] = keyed_rotation(curves, -1)

    off = loop_mismatch(first, last)
    if off:
        worst = ", ".join(f"{bone} {angle:.0f} deg" for bone, angle in off[:4])
        print(
            f"warning: {action.name} does not end where it starts "
            f"({len(off)} bone(s) apart: {worst}), so looping it will hitch"
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
    roles = read_skeleton(rig_path)
    canonical = import_armature(rig_path)
    keep_only(canonical)

    canonical_bones = [bone.name for bone in canonical.data.bones]
    if absent := unfilled_roles(roles.convention(roles.canonical), canonical_bones):
        sys.exit(
            f"error: the canonical rig {rig_path.name} has no bone for {absent}, "
            f"which {rig_path.with_suffix('.toml').name} says it must"
        )

    known_actions = set(bpy.data.actions)
    source = import_armature(source_path)
    action = sole_action(known_actions, name)
    source_bones = [bone.name for bone in source.data.bones]
    if absent := unfilled_roles(roles.convention(convention), source_bones):
        sys.exit(
            f"error: {source_path.name} has no bone for {len(absent)} role(s) "
            f"under the {convention!r} naming convention, {absent[:6]}, so those "
            f"would go undriven. Its bones are named like {sorted(source_bones)[:4]}"
        )
    matched = matched_bones(source, roles.bone_map(convention))

    if dropped := drop_unmatched_curves(action, matched):
        print(
            f"dropped {len(dropped)} channel(s) with no role on the canonical "
            f"rig: {dropped[:6]}"
        )
    rename_to_canonical(action, matched)
    rebase_rotations(action, source, canonical, matched)
    ratio = translation_scale(
        rest_height(rest_points(source)), rest_height(rest_points(canonical))
    )
    scale_translation(action, ratio)
    report_loop(action, sorted(matched.values()))

    keep_only(canonical)
    assign_action(canonical, action)
    export(out, canonical)
    print(
        f"retargeted {name}: {len(matched)} bone(s) onto {rig_path.name}, "
        f"translation sized by {ratio:.4f} -> {out}"
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
