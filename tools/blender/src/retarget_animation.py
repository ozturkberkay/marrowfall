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
        --out art/animations/local/walk_back.glb --name walk_back \
        --source-motion art/staging/reports/retarget.walk_back.1.source.json \
        --source-fps 30 --travels true --limit clip.fps_grid=0.0001
"""

import argparse
import pathlib
import sys

import bpy
import plant
from actions import (
    action_fcurves,
    assign_action,
    bone_basis,
    location_curves,
    scale_translation,
)
from armature import export, keep_only
from clip import (
    FPS_GRID,
    FPS_GRID_RANGE,
    Channel,
    Fit,
    Grid,
    counted,
    floor_lift,
    ground_from,
    on_the_grid,
    placed,
    source_motion,
    whole_range,
)
from findings import Finding, guard, limits_from, write_report
from framing import Vec3, translation_scale
from mathutils import Matrix, Vector
from plant import Leg
from skeleton import Skeleton, bare_bone_name, unfilled_roles

# `travel` is the same measurement `source.traveling` took at the fetch
# boundary, so the two cannot mean different things by the word.
from source import travel
from transfer import (
    Bone,
    LocalPose,
    Mat4,
    child_basis,
    mat_translation,
    offsets,
    quat_degrees,
    re_rolled,
    reference_pose,
    segment_length,
    transfer,
)


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
        "--source-motion",
        type=pathlib.Path,
        required=True,
        help=(
            "Where to record the source clip's own world orientations. "
            "`clip.swing` and `clip.twist` read them beside the output GLB, "
            "because the vendor file is an FBX no Rust reader opens."
        ),
    )
    parser.add_argument(
        "--name", required=True, help="Library name, which the action is stored under."
    )
    parser.add_argument(
        "--source-fps",
        type=int,
        required=True,
        help=(
            "The clip's own rate, which the scene is set to. glTF stores key "
            "times in seconds, so the scene's rate decides which frames they "
            "land on, and `clip.fps_grid` reports that they landed whole."
        ),
    )
    parser.add_argument(
        "--travels",
        required=True,
        choices=["true", "false"],
        help=(
            "What the library declares about the source's root motion. "
            "`clip.stride` reads a clip that travels against the source's own "
            "travel, and reports itself switched off for one that does not."
        ),
    )
    parser.add_argument(
        "--children",
        required=True,
        help=(
            "ROLE=CHILD pairs, comma separated: which role each bone's own "
            "axis points at. The source's bones are re-rolled onto these "
            "before either rig is aimed. The Rust side reads them off "
            "`[profile.tails]`."
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


def children_from(entry: str) -> dict[str, str]:
    """`hips=spine_lower,neck=head` into the map `child_basis` reads.

    Here and not in `check_source.py` because both scripts take the same
    table: the source gates report how far a vendor bone sits from its own
    child, and the retarget is what takes that difference out.
    """
    pairs = {}
    for pair in entry.split(","):
        role, _, child = pair.partition("=")
        if not role or not child:
            sys.exit(f"error: --children needs ROLE=CHILD pairs, got {pair!r}")
        pairs[role] = child
    return pairs


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


def source_action(known: set[bpy.types.Action], path: pathlib.Path) -> bpy.types.Action:
    """The one action the source file brought, which says which frames exist."""
    fresh = [action for action in bpy.data.actions if action not in known]
    if not fresh:
        sys.exit(f"error: {path.name} contains no animation")
    if len(fresh) > 1:
        names = [action.name for action in fresh]
        sys.exit(f"error: {path.name} holds {len(names)} actions, expected 1: {names}")
    return fresh[0]


def source_frames(action: bpy.types.Action) -> range:
    """The frames the source is sampled at, rounded to the scene's grid.

    Rounded, and reported rather than refused: `clip.fps_grid` measures every
    key against a whole frame and `clip.fps_grid.range` measures the render
    range, so a clip read at the wrong rate leaves a numbered defect per key
    instead of an exit nobody can read afterwards.
    """
    lo, hi = action.frame_range
    return range(round(lo), round(hi) + 1)


def source_grid(action: bpy.types.Action) -> Grid:
    """Where the source's keys really sit, on the scene's own frame grid.

    Read before the action is removed, which the retarget does so the
    exporter cannot write the vendor's own action out beside ours.
    """
    lo, hi = action.frame_range
    return Grid(
        keys=tuple(
            key.co[0]
            for curve in action_fcurves(action)
            for key in curve.keyframe_points
        ),
        span=(lo, hi),
    )


def set_rate(rate: int) -> None:
    """Runs the scene at the clip's own rate.

    Set twice: before the source is imported, because the glTF importer turns
    seconds into frames using whatever rate the scene is on, and again after,
    because the FBX importer sets the scene from the file and would otherwise
    decide this for us. `fps_base` is pinned to 1 because nothing in the
    library declares a fractional rate.
    """
    scene = bpy.context.scene
    scene.render.fps = rate
    scene.render.fps_base = 1.0


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


def measure(
    fitted: bpy.types.Action,
    grid: Grid,
    frames: range,
    fit: Fit,
    limits: dict[str, float],
) -> list[Finding]:
    """What this run reports: requirement 9 per bone, requirement 3 per key,
    and requirement 4 on where the clip ended up.

    Every one is built in `clip.py`, which imports no `bpy` and has its own
    negatives, so the severity is never decided here.
    """
    scene = bpy.context.scene
    return [
        *counted(channels(fitted), frames),
        *on_the_grid(grid.keys, scene.render.fps, FPS_GRID.at(limits)),
        whole_range(frames, grid.keys, FPS_GRID_RANGE.at(limits)),
        *placed(fit, limits),
    ]


def rig_height(armature: bpy.types.Object) -> float:
    """The rig's own joint span along world up, in meters.

    What the contact thresholds are scaled by: they are published against a
    180 cm reference, and a rig is whatever height it was rigged at. Blender's
    glTF importer creates exactly one bone per skin joint, so these are the
    same bones `check/gltf_clip.rs::joint_span` spans on the same file.
    """
    up = [
        (armature.matrix_world @ bone.matrix_local).to_translation().z
        for bone in armature.data.bones
    ]
    return max(up) - min(up)


def leg_bones(armature: bpy.types.Object, toe: str) -> list[str]:
    """The toe and the four bones above it, root first.

    The armature's own hierarchy, because that is the chain a pose composes
    along. A toe hanging from fewer than four bones has no leg to solve.
    """
    chain = [toe]
    bone = armature.data.bones[toe]
    for _ in range(4):
        if bone.parent is None:
            sys.exit(
                f"error: {toe} has {len(chain) - 1} bone(s) above it and a leg "
                f"needs four, so no foot plant can move it"
            )
        bone = bone.parent
        chain.append(bone.name)
    return list(reversed(chain))


def sole_under(rest: Matrix) -> Vector:
    """Where the ground sits beneath one joint, in that joint's own frame.

    The rig's rest pose stands on the floor, so the sole under a joint is
    that joint carried straight down to zero at rest. Held in the joint's own
    frame, it then moves rigidly with the foot.
    """
    head = rest.to_translation()
    return rest.inverted() @ Vector((head.x, head.y, 0.0))


def posed_leg(
    armature: bpy.types.Object, chain: list[str], rest: dict[str, Matrix]
) -> Leg:
    """One leg at the scene's current frame, as `plant.bend` reads it."""
    world = [armature.matrix_world @ armature.pose.bones[b].matrix for b in chain[:4]]
    return Leg(
        rest=tuple(tuple(rest[bone].to_quaternion()) for bone in chain[:4]),
        pose=tuple(tuple(matrix.to_quaternion()) for matrix in world),
        joints=tuple(tuple(matrix.to_translation()) for matrix in world[1:4]),
    )


def sole_paths(
    armature: bpy.types.Object, toes: list[str], frames: range
) -> dict[str, list[Vec3]]:
    """Every foot's two sole points over the clip, keyed by the joint each
    one hangs under.

    What the floor snap stands the clip on, and the same two points
    `plant.py` reads contact and penetration on.
    """
    chains = {toe: leg_bones(armature, toe) for toe in toes}
    bones = [bone for chain in chains.values() for bone in chain[3:5]]
    rest = {
        bone: armature.matrix_world @ armature.data.bones[bone].matrix_local
        for bone in bones
    }
    sole = {bone: sole_under(matrix) for bone, matrix in rest.items()}
    paths: dict[str, list[Vec3]] = {bone: [] for bone in bones}
    scene = bpy.context.scene
    for frame in frames:
        scene.frame_set(frame)
        for bone in bones:
            world = armature.matrix_world @ armature.pose.bones[bone].matrix
            paths[bone].append(tuple(world @ sole[bone]))
    return paths


def read_legs(
    armature: bpy.types.Object,
    legs: dict[str, list[str]],
    rest: dict[str, Matrix],
    sole: dict[str, Vector],
    frames: range,
) -> dict[str, tuple[list[Vec3], list[Vec3], list[Leg]]]:
    """Each foot's two sole points and each leg's pose, frame by frame.

    Evaluated rather than derived: where a foot lands is not something the
    keys say on their own.
    """
    read: dict[str, tuple[list[Vec3], list[Vec3], list[Leg]]] = {
        toe: ([], [], []) for toe in legs
    }
    scene = bpy.context.scene
    for frame in frames:
        scene.frame_set(frame)
        for toe, chain in legs.items():
            ball, heel, posed = read[toe]
            for point, bone in ((ball, chain[4]), (heel, chain[3])):
                world = armature.matrix_world @ armature.pose.bones[bone].matrix
                point.append(tuple(world @ sole[bone]))
            posed.append(posed_leg(armature, chain, rest))
    return read


def hold_still(
    armature: bpy.types.Object,
    chain: list[str],
    legs: list[Leg],
    held: list[Vec3],
    ball: list[Vec3],
    frames: range,
) -> float:
    """Keys the three leg bones so each frame's foot lands where `held` says.

    Returns the worst distance the leg could not reach, which is a leg asked
    to stretch past its own length. `plant.py` decides all of it.
    """
    worst = 0.0
    for at, frame in enumerate(frames):
        move = (held[at][0] - ball[at][0], held[at][1] - ball[at][1], 0.0)
        reach = plant.bend(legs[at], move)
        worst = max(worst, reach.shortfall)
        for bone, rotation in zip(chain[1:4], plant.keys(legs[at], reach), strict=True):
            posed = armature.pose.bones[bone]
            posed.rotation_quaternion = rotation
            posed.keyframe_insert("rotation_quaternion", frame=frame)
    return worst


def foot_plant(
    armature: bpy.types.Object,
    toes: list[str],
    frames: range,
    source_fps: int,
    travels: bool,
    limits: dict[str, float],
) -> tuple[list[Finding], float]:
    """Holds every planted foot still, then reports what is left under it.

    Read back after the keys are written, so the three findings are what the
    clip carries rather than what the lock was asked for.
    """
    legs = {toe: leg_bones(armature, toe) for toe in toes}
    rest = {
        bone: armature.matrix_world @ armature.data.bones[bone].matrix_local
        for chain in legs.values()
        for bone in chain
    }
    sole = {bone: sole_under(matrix) for bone, matrix in rest.items()}
    scale = plant.scale_of(rig_height(armature))

    read = read_legs(armature, legs, rest, sole, frames)
    runs, worst = {}, 0.0
    for toe, chain in legs.items():
        ball, _, posed = read[toe]
        runs[toe] = plant.plant_runs(ball, source_fps, scale)
        held = plant.locked(ball, runs[toe], travels, plant.SKATE.at(limits).limit)
        worst = max(worst, hold_still(armature, chain, posed, held, ball, frames))

    findings = []
    for toe, (ball, heel, _) in read_legs(armature, legs, rest, sole, frames).items():
        stood = [(frames[start], frames[end]) for start, end in runs[toe]]
        findings.append(plant.plants(toe, stood, travels, plant.PLANTS.at(limits)))
        findings.extend(
            plant.skate(
                toe,
                stood,
                [plant.drift(ball, run) for run in runs[toe]],
                travels,
                plant.SKATE.at(limits),
            )
        )
        findings.append(
            plant.penetration(
                toe,
                [point[2] for point in ball],
                [point[2] for point in heel],
                list(frames),
                plant.PENETRATION.at(limits),
            )
        )
    return findings, worst


def world_heads(
    armature: bpy.types.Object, bones: list[str], frames: range
) -> dict[str, list[Vec3]]:
    """Where each bone's head sits at every frame, in world space.

    The pose is evaluated frame by frame, which is the one thing the transfer
    never does: where a foot lands is not something the keys say on their own.
    """
    scene = bpy.context.scene
    tracked: dict[str, list[Vec3]] = {bone: [] for bone in bones}
    for frame in frames:
        scene.frame_set(frame)
        for bone, path in tracked.items():
            head = armature.matrix_world @ armature.pose.bones[bone].matrix
            path.append(tuple(head.to_translation()))
    return tracked


def lift_root(
    armature: bpy.types.Object, action: bpy.types.Action, bone: str, lift: float
) -> None:
    """Moves every root location key by `lift` meters of world height.

    In world space, because the root's own channels are not world axes: on
    this rig `Hips` local Z is world minus Z tilted 8.9 degrees, the same
    reason `strip_root_motion` pins in world space.
    """
    curves = location_curves(action, bone)
    if not curves:
        sys.exit(
            f"error: {bone} carries no location key, so nothing can lift "
            f"{action.name} onto the floor"
        )
    step = bone_basis(armature, bone).inverted() @ Vector((0.0, 0.0, lift))
    for curve, value in zip(curves, step, strict=True):
        for point in curve.keyframe_points:
            point.co[1] += value
            point.handle_left[1] += value
            point.handle_right[1] += value
        curve.update()


def retarget(args: argparse.Namespace) -> None:
    source_path, rig_path, out = args.source, args.rig, args.out
    bpy.ops.wm.read_factory_settings(use_empty=True)
    # Before the imports: the glTF importer turns key times in seconds into
    # frames using whatever rate the scene is on.
    set_rate(args.source_fps)
    skeleton = read_skeleton(rig_path)
    ours = import_armature(rig_path)
    keep_only(ours)
    our_roles = refuse_unfilled(rig_path, ours, skeleton, skeleton.canonical)

    known = set(bpy.data.actions)
    source = import_armature(source_path)
    # And again after: the FBX importer sets the scene from the file, which
    # would leave the rate to the vendor rather than to the library.
    set_rate(args.source_fps)
    action = source_action(known, source_path)
    source_roles = refuse_unfilled(source_path, source, skeleton, args.convention)

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
    # The vendor's own axis convention, taken out before anything is aimed: a
    # bone whose own axis is not the direction to its child gets a reference
    # pose that is not the same BODY pose as ours, and the difference lands in
    # every key. Ours is not re-rolled, because `rig.child_axis` holds it to
    # 2 degrees and the clip gates state its bone frames.
    rest_source = rest_in_world(source, source_roles)
    basis = child_basis(rest_source, children_from(args.children))
    rest_source = re_rolled(rest_source, basis)
    driven_basis = {role: basis[role] for role in driven}
    offset = offsets(
        reference_pose({r: rest_ours[r] for r in driven}, chain, aim),
        reference_pose({r: rest_source[r] for r in driven}, chain, aim),
    )

    bones = target_bones(ours, our_roles, set(driven))
    object_matrix = as_mat4(ours.matrix_world)
    frames = source_frames(action)
    grid = source_grid(action)
    poses = {}
    source_world = {}
    for frame in frames:
        bpy.context.scene.frame_set(frame)
        # Kept, not only passed on: `clip.swing` and `clip.twist` measure the
        # exported GLB against this, and nothing downstream can reopen an FBX.
        source_world[frame] = re_rolled(pose_in_world(source, driven), driven_basis)
        poses[frame] = transfer(bones, object_matrix, source_world[frame], offset)
    segment = (
        segment_length(rest_ours, *skeleton.stride_segment),
        segment_length(rest_source, *skeleton.stride_segment),
    )
    ratio = translation_scale(segment[1], segment[0])
    top = skeleton.chain_top
    source_travel = travel(tuple(mat_translation(source_world[f][top]) for f in frames))
    source_motion(
        {role: rest_source[role] for role in driven},
        source_world,
        bpy.context.scene.render.fps,
        bpy.context.scene.render.fps_base,
        source_travel,
        segment[1],
    ).write(args.source_motion)

    keep_only(ours)
    # The source's own action outlives its armature, and the exporter would
    # write it into the GLB beside ours.
    bpy.data.actions.remove(action)
    fitted = write_keys(ours, args.name, poses)
    scale_translation(fitted, ratio)

    root = our_roles[top]
    toes = [our_roles[role] for role in skeleton.ground_roles]
    lift = floor_lift(ground_from(sole_paths(ours, toes, frames), frames))
    lift_root(ours, fitted, root, lift)
    limits = limits_from(args.limit)
    standing, unreached = foot_plant(
        ours, toes, frames, args.source_fps, args.travels == "true", limits
    )
    linear_and_constant(fitted)

    # Read back after the lift and the plant, so the report is what the file
    # carries rather than what either was asked for.
    tracked = world_heads(ours, [root], frames)
    fit = Fit(
        name=args.name,
        ground=tuple(ground_from(sole_paths(ours, toes, frames), frames)),
        travel=(travel(tuple(tracked[root])), source_travel),
        segment=segment,
        ratio=ratio,
        travels=args.travels == "true",
    )
    write_report([*measure(fitted, grid, frames, fit, limits), *standing])
    export(out, ours)

    worst = max(driven, key=lambda role: quat_degrees(offset[role]))
    print(
        f"retargeted {args.name}: {len(driven)} role(s) onto {rig_path.name} over "
        f"{len(frames)} frame(s), worst offset {worst} "
        f"{quat_degrees(offset[worst]):.2f} deg, lengths sized by {ratio:.4f}, "
        f"lifted {lift:.4f} m onto the floor, feet held to within "
        f"{unreached:.4f} m of where the plant asked -> {out}"
    )


def main() -> None:
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    retarget(parse_args(argv))


def save_blend(path: pathlib.Path) -> None:
    """The scene as it stood when the script failed, for a human to open."""
    bpy.ops.wm.save_as_mainfile(filepath=str(path), copy=True)


if __name__ == "__main__":
    # Blender exits 0 on an uncaught exception, so a failed fit would look like
    # success and leave a clip nobody can bake. `guard` writes the success
    # sentinel the Rust side asserts.
    guard(main, save_blend)
