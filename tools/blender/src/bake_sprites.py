"""Bake a rigged GLB into directional isometric sprite sheets.

This is the Diablo-II method: render a 3D character from a fixed isometric
camera through N compass directions x every animation, and ship the resulting
2D frames. The mesh never reaches the game, only the pixels do.

Blender is scripted in Python because that is the only way it can be scripted:
its CLI can render a `.blend` someone already authored, but importing a GLB,
sizing an orthographic camera and sampling an armature are all `bpy` calls, and
`bpy` exists only inside Blender's own interpreter. `cargo art` launches this.

All geometry and scheduling lives in `framing`, which never imports `bpy` and is
therefore unit tested. This module is the Blender half: it reads numbers out of
the scene, hands them over, and applies the answers.

Run headless (`cargo art` does this for you, and sets PYTHONPATH so the
project's pydantic is importable):

    # 1. See what is actually inside a GLB (do this first)
    blender --background --python-use-system-env \
        --python tools/blender/src/bake_sprites.py -- \
        --glb art/characters/survivor/model.glb --inspect

    # 2. Bake: the character once, plus one animation-only file each
    blender --background --python-use-system-env \
        --python tools/blender/src/bake_sprites.py -- \
        --character art/characters/survivor/model.glb \
        --out art/staging/survivor \
        --animation idle=art/characters/survivor/animations/idle.glb \
        --animation run=art/characters/survivor/animations/run.glb

Output: <out>/<animation>_<direction>_<frame>.png. Packing them into atlases is
`cargo art`'s job.

Conventions:
  - Camera is ORTHOGRAPHIC at 35 degrees elevation, the tile grid is 2:1
    dimetric, and the character must be drawn to the same projection.
  - The CHARACTER rotates and the camera/lights stay fixed, so the key light
    always falls from screen upper-left regardless of facing.
  - Direction 0 is the character facing the camera (screen south), then
    counter-clockwise.
"""

import argparse
import math
import sys
import traceback
from pathlib import Path

import bpy
from framing import (
    BIND_POSE_TOLERANCE_DEG,
    BakeSettings,
    Bounds,
    Framing,
    bind_pose_mismatch,
    bone_from_data_path,
    direction_rotation,
    forearm_roll_sign,
    frame_filename,
    is_forearm,
    key_light_rotation,
    missing_bones,
    sampled_frames,
)
from mathutils import Quaternion, Vector
from pydantic import BaseModel, ConfigDict


class Character(BaseModel):
    """An imported character: one armature and the meshes skinned to it."""

    # bpy objects are opaque to pydantic; they are still validated as instances.
    model_config = ConfigDict(arbitrary_types_allowed=True, frozen=True)

    armature: bpy.types.Object
    meshes: list[bpy.types.Object]


class Animation(BaseModel):
    """One action to bake, under the name the game will use for it."""

    model_config = ConfigDict(arbitrary_types_allowed=True, frozen=True)

    action: bpy.types.Action
    name: str


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--character",
        type=Path,
        help="GLB holding the mesh, skeleton and textures, with no animation. "
        "Pair with --animation. The mesh is stored once and shared.",
    )
    parser.add_argument(
        "--animation",
        action="append",
        default=[],
        metavar="NAME=PATH",
        help="Animation-only GLB (armature + one action, no mesh), baked under "
        "NAME. Repeatable. Its action is moved onto --character's armature.",
    )
    parser.add_argument("--out", type=Path, default=Path("art/staging/out"))
    parser.add_argument(
        "--glb", type=Path, help="A GLB to report on with --inspect. Diagnostics only."
    )
    parser.add_argument(
        "--inspect",
        action="store_true",
        help="Print --glb's actions, bones and bounds, then exit without rendering.",
    )
    parser.add_argument("--directions", type=int, default=8)
    parser.add_argument(
        "--fps",
        action="append",
        default=[],
        metavar="NAME=RATE",
        help="Sprite frames sampled per second, once per animation. Frame count "
        "follows from each animation's own duration, so a clip keeps its "
        "authored speed whatever rate it is sampled at.",
    )
    parser.add_argument("--size", type=int, default=256)
    parser.add_argument(
        "--trim-start",
        type=float,
        default=0.0,
        help="Fraction of each animation to skip at the start, for generated "
        "motions that ramp in from a neutral pose (e.g. 0.25).",
    )
    parser.add_argument(
        "--keep-root-motion",
        action="store_true",
        help="Leave the root bone's translation intact. By default it is "
        "removed, because the game moves the character and a travelling "
        "animation slides out of frame. Diagnostics only.",
    )
    parser.add_argument(
        "--forearm-roll",
        type=float,
        default=0.0,
        help="Degrees to roll the forearm bones, correcting a palms-forward "
        "bind pose. Positive rotates palms inward.",
    )
    return parser.parse_args(argv)


def settings_from(args: argparse.Namespace) -> BakeSettings:
    """Validates the raw command line into the checked settings object."""
    return BakeSettings(
        directions=args.directions,
        fps=parse_rates(args.fps),
        size=args.size,
        trim_start=args.trim_start,
        forearm_roll=args.forearm_roll,
    )


def clear_scene() -> None:
    bpy.ops.wm.read_factory_settings(use_empty=True)


def import_glb(path: Path) -> Character:
    """Imports the GLB and returns its armature and character meshes.

    Generated exports can carry helper geometry alongside the character (an
    Icosphere, for instance). Only skinned meshes are part of the character, so
    anything without an armature modifier is deleted, otherwise it both renders
    into the sprite and corrupts the camera framing.
    """
    if not path.exists():
        sys.exit(f"error: {path} not found")
    bpy.ops.import_scene.gltf(filepath=str(path))

    armature = next((o for o in bpy.data.objects if o.type == "ARMATURE"), None)
    if armature is None:
        sys.exit("error: no armature in GLB, re-export with rigging enabled")
    all_meshes = [o for o in bpy.data.objects if o.type == "MESH"]
    if not all_meshes:
        sys.exit("error: GLB contains no mesh")

    skinned = [
        m for m in all_meshes if any(mod.type == "ARMATURE" for mod in m.modifiers)
    ]
    if skinned:
        for stray in (m for m in all_meshes if m not in skinned):
            print(f"discarding non-character mesh: {stray.name}")
            bpy.data.objects.remove(stray, do_unlink=True)
        return Character(armature=armature, meshes=skinned)
    return Character(armature=armature, meshes=all_meshes)


def evaluated_bounds(meshes: list[bpy.types.Object]) -> Bounds:
    """World-space min/max of the meshes as currently posed.

    Reads the evaluated (post-modifier) mesh so the armature deformation is
    included, object bound_box reflects the rest pose only.
    """
    depsgraph = bpy.context.evaluated_depsgraph_get()
    lo = [math.inf] * 3
    hi = [-math.inf] * 3
    for obj in meshes:
        evaluated = obj.evaluated_get(depsgraph)
        mesh = evaluated.to_mesh()
        for vertex in mesh.vertices:
            world = evaluated.matrix_world @ vertex.co
            for axis in range(3):
                lo[axis] = min(lo[axis], world[axis])
                hi[axis] = max(hi[axis], world[axis])
        evaluated.to_mesh_clear()
    return Bounds(lo=(lo[0], lo[1], lo[2]), hi=(hi[0], hi[1], hi[2]))


def measure_framing(
    character: Character, animations: list[Animation], settings: BakeSettings
) -> Framing:
    """Vertical span and turning radius over every pose that will be rendered.

    Two things make rest-pose framing wrong. A running character reaches
    further than a standing one, and the character *spins* about the axis for
    the direction ring, so what must fit is the radius swept about that axis,
    not the extent in any single facing.
    """
    lo_z, hi_z = math.inf, -math.inf
    radius = 0.0

    for animation in animations:
        assign_action(character.armature, animation.action)
        start, end = animation.action.frame_range
        for frame in sampled_frames(
            start,
            end,
            bpy.context.scene.render.fps,
            settings.fps[animation.name],
            settings.trim_start,
        ):
            bpy.context.scene.frame_set(frame)
            depsgraph = bpy.context.evaluated_depsgraph_get()
            for obj in character.meshes:
                evaluated = obj.evaluated_get(depsgraph)
                mesh = evaluated.to_mesh()
                for vertex in mesh.vertices:
                    world = evaluated.matrix_world @ vertex.co
                    lo_z = min(lo_z, world.z)
                    hi_z = max(hi_z, world.z)
                    radius = max(radius, math.hypot(world.x, world.y))
                evaluated.to_mesh_clear()
    return Framing(lo_z=lo_z, hi_z=hi_z, radius=radius)


def inspect(character: Character) -> None:
    """Dumps everything needed to configure a bake."""
    print("\n=== GLB CONTENTS ===")

    print(f"\nmeshes ({len(character.meshes)}):")
    for mesh in character.meshes:
        print(f"  {mesh.name}: {len(mesh.data.polygons)} faces")

    bones = character.armature.data.bones
    print(f"\narmature: {character.armature.name} ({len(bones)} bones)")
    print(f"  roots: {[b.name for b in bones if b.parent is None]}")
    arms = [b.name for b in bones if is_forearm(b.name) or "hand" in b.name.lower()]
    print(f"  arm/hand bones: {arms}")

    print(f"\nactions ({len(bpy.data.actions)}):")
    for action in bpy.data.actions:
        start, end = (int(v) for v in action.frame_range)
        print(f'  "{action.name}"  frames {start}-{end}')
    if not bpy.data.actions:
        print("  NONE, was the animation exported?")

    bounds = evaluated_bounds(character.meshes)
    print("\nrest-pose bounds (Blender axes, Z up):")
    print(f"  min {tuple(round(v, 3) for v in bounds.lo)}")
    print(f"  max {tuple(round(v, 3) for v in bounds.hi)}")
    print(f"  size {tuple(round(v, 3) for v in bounds.size)}")
    print(f"  height {bounds.height:.3f}")
    print("\n=== END ===\n")


def setup_camera(framing: Framing) -> bpy.types.Object:
    """Orthographic camera at the isometric elevation, framing the character."""
    camera_data = bpy.data.cameras.new("iso_cam")
    camera_data.type = "ORTHO"
    camera_data.ortho_scale = framing.ortho_scale

    camera = bpy.data.objects.new("iso_cam", camera_data)
    camera.location = Vector(framing.camera_location)
    camera.rotation_euler = framing.camera_rotation

    bpy.context.collection.objects.link(camera)
    bpy.context.scene.camera = camera
    return camera


def setup_lighting() -> None:
    """Key light from screen upper-left plus a soft ambient fill."""
    key_data = bpy.data.lights.new("key", type="SUN")
    key_data.energy = 3.0
    key_data.angle = math.radians(15.0)  # soft-edged shadows
    key = bpy.data.objects.new("key", key_data)
    key.rotation_euler = key_light_rotation()
    bpy.context.collection.objects.link(key)

    # Ambient fill via world colour, lifts shadows so detail stays legible
    # once the sprite is downscaled and composited over a dark tile.
    world = bpy.data.worlds.new("world")
    world.use_nodes = True
    background = world.node_tree.nodes["Background"]
    background.inputs[0].default_value = (0.27, 0.29, 0.32, 1.0)
    background.inputs[1].default_value = 0.4
    bpy.context.scene.world = world


def setup_render(size: int) -> None:
    scene = bpy.context.scene
    # EEVEE is plenty for flat sprite work and far faster than Cycles; the
    # identifier moved around across Blender versions, so pick what exists.
    engines = {
        item.identifier
        for item in scene.bl_rna.properties["render"]
        .fixed_type.bl_rna.properties["engine"]
        .enum_items
    }
    for engine in ("BLENDER_EEVEE_NEXT", "BLENDER_EEVEE", "CYCLES"):
        if engine in engines:
            scene.render.engine = engine
            break

    scene.render.resolution_x = size
    scene.render.resolution_y = size
    scene.render.resolution_percentage = 100
    scene.render.film_transparent = True  # alpha, so sprites composite over tiles
    scene.render.image_settings.file_format = "PNG"
    scene.render.image_settings.color_mode = "RGBA"


def apply_forearm_roll(armature: bpy.types.Object, degrees: float) -> None:
    """Corrects a palms-forward bind pose across every action.

    The roll is composed into the animated rotation curves rather than set on
    the pose bone: glTF actions drive `rotation_quaternion`, so writing
    `rotation_euler` (or switching rotation_mode) would be overwritten by the
    action at best, and freeze the bone at worst.
    """
    if not degrees:
        return
    matched = [b.name for b in armature.pose.bones if is_forearm(b.name)]
    if not matched:
        print(f"warning: --forearm-roll {degrees} given but no forearm bones matched")
        return

    for action in bpy.data.actions:
        for name in matched:
            # Roll about the bone's own length axis (Y in Blender bone space).
            sign = forearm_roll_sign(name)
            roll = Quaternion((0.0, 1.0, 0.0), math.radians(degrees * sign))
            path = f'pose.bones["{name}"].rotation_quaternion'
            curves = [fc for fc in action_fcurves(action) if fc.data_path == path]
            if len(curves) != 4:
                continue
            curves.sort(key=lambda fc: fc.array_index)
            for index in range(len(curves[0].keyframe_points)):
                current = Quaternion([c.keyframe_points[index].co[1] for c in curves])
                rolled = current @ roll
                for channel, value in zip(curves, rolled, strict=True):
                    channel.keyframe_points[index].co[1] = value
                    channel.keyframe_points[index].handle_left[1] = value
                    channel.keyframe_points[index].handle_right[1] = value
            for channel in curves:
                channel.update()
    print(f"applied {degrees} deg forearm roll to {matched}")


def assign_action(armature: bpy.types.Object, action: bpy.types.Action) -> None:
    """Makes `action` drive `armature`.

    Blender 4.4+ actions are slotted: the action alone is not enough, a slot
    must be bound or the armature simply does not move. An action imported
    alongside its own armature arrives pre-bound; one moved in from a different
    file does not, so the binding has to be made explicitly.
    """
    armature.animation_data_create()
    data = armature.animation_data
    data.action = action

    if not hasattr(data, "action_slot"):
        return  # pre-4.4 Blender: assigning the action is sufficient

    candidates = list(getattr(data, "action_suitable_slots", []) or [])
    if not candidates:
        candidates = list(getattr(action, "slots", []) or [])
    if candidates:
        data.action_slot = candidates[0]
    elif hasattr(action, "slots"):
        # A slot-less action animates nothing; give it one bound to this rig.
        slot = action.slots.new(id_type="OBJECT", name=armature.name)
        data.action_slot = slot


def action_fcurves(action: bpy.types.Action) -> list[bpy.types.FCurve]:
    """Every F-curve in an action, across Blender's two action APIs.

    Blender 4.4 introduced slotted actions, where curves live under
    layers/strips/channelbags; older files expose `action.fcurves` directly.
    """
    if hasattr(action, "fcurves"):
        return list(action.fcurves)
    curves: list[bpy.types.FCurve] = []
    for layer in action.layers:
        for strip in layer.strips:
            for channelbag in getattr(strip, "channelbags", ()):
                curves.extend(channelbag.fcurves)
    return curves


def strip_root_motion(armature: bpy.types.Object) -> None:
    """Pins the root bone in place across every action.

    Library animations usually travel: a walk-backward moves along -Y. The game
    moves the character itself, so a travelling animation would slide out of
    frame, and because the camera is tilted, horizontal travel projects onto
    the *vertical* screen axis too, inflating the crop for every frame.

    Only the horizontal channels are pinned. Flattening the vertical one too
    would delete the run cycle's bob and leave a jump permanently on the
    ground, that is animation, not travel.

    Translation is flattened to its first-frame value rather than zeroed, so the
    character keeps whatever offset the rig was authored with.
    """
    roots = [bone.name for bone in armature.pose.bones if bone.parent is None]
    if not roots:
        return

    for action in bpy.data.actions:
        for name in roots:
            path = f'pose.bones["{name}"].location'
            for curve in (fc for fc in action_fcurves(action) if fc.data_path == path):
                # Index 2 is the vertical channel. Measured, not assumed: the
                # Hips rest matrix maps local Z to world Z on this rig, and a
                # run's bob shows up there (0.095 units) while its horizontal
                # travel does not (0.029 and 0.011).
                if curve.array_index == 2:
                    continue
                points = curve.keyframe_points
                if not points:
                    continue
                anchor = points[0].co[1]
                for point in points:
                    point.co[1] = anchor
                    point.handle_left[1] = anchor
                    point.handle_right[1] = anchor
                curve.update()
    print(f"pinned root motion on {roots}")


def bone_directions(
    armature: bpy.types.Object,
) -> dict[str, tuple[float, float, float]]:
    """Each bone's rest direction, for comparing one rig's bind pose to another."""
    directions = {}
    for bone in armature.data.bones:
        delta = bone.tail_local - bone.head_local
        if delta.length > 0:
            delta = delta.normalized()
            directions[bone.name] = (delta.x, delta.y, delta.z)
    return directions


def take_action(path: Path, target: bpy.types.Object, name: str) -> bpy.types.Action:
    """Loads one action out of `path` and hands it to `target`'s armature.

    Animation-only files still carry an armature, because glTF animations target
    nodes inside their own file, the format has no cross-file reference. The
    imported armature is therefore thrown away after its action has been taken;
    bone names match, so the action drives the character's own skeleton.
    """
    if not path.exists():
        sys.exit(f"error: animation {path} not found")

    known_actions = set(bpy.data.actions)
    known_objects = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=str(path))

    imported = [o for o in bpy.data.objects if o not in known_objects]
    new_actions = [a for a in bpy.data.actions if a not in known_actions]
    source_rig = next((o for o in imported if o.type == "ARMATURE"), None)
    if not new_actions:
        sys.exit(f"error: {path} contains no animation")
    if len(new_actions) > 1:
        names = [a.name for a in new_actions]
        sys.exit(f"error: {path} holds {len(names)} actions, expected 1: {names}")

    action = new_actions[0]
    action.name = name
    if source_rig is None:
        sys.exit(
            f"error: {path} has no armature, so its bind pose cannot be checked "
            "against the character's. Re-download it."
        )
    source_bind = bone_directions(source_rig)
    # Discard the imported skeleton; only its motion was wanted.
    for obj in imported:
        bpy.data.objects.remove(obj, do_unlink=True)

    animated = {
        bone
        for curve in action_fcurves(action)
        if (bone := bone_from_data_path(curve.data_path)) is not None
    }
    missing = missing_bones(animated, {b.name for b in target.pose.bones})
    if missing:
        sys.exit(
            f"error: {path} animates bones absent from the character: "
            f"{missing[:5]}, the animation and the character come from "
            "different rigs"
        )
    if off := bind_pose_mismatch(bone_directions(target), source_bind):
        worst = ", ".join(f"{name} {angle:.0f} deg" for name, angle in off[:4])
        sys.exit(
            f"error: {path} was built for a rig in a different bind pose "
            f"({len(off)} bone(s) over {BIND_POSE_TOLERANCE_DEG:.0f} deg: {worst}). "
            "An action holds rotations relative to its own rest pose, so this "
            "would flail. Re-buy the animation against this character's rig."
        )
    return action


def parent_to_pivot() -> bpy.types.Object:
    """Parents the scene to a pivot, so rotating it turns the character.

    Rotating a pivot rather than the rig leaves the character's own transforms
    and its animation data untouched.
    """
    pivot = bpy.data.objects.new("pivot", None)
    bpy.context.collection.objects.link(pivot)
    for obj in bpy.data.objects:
        if (
            obj.parent is None
            and obj not in (pivot, bpy.context.scene.camera)
            and obj.type != "LIGHT"
        ):
            obj.parent = pivot
            obj.matrix_parent_inverse = pivot.matrix_world.inverted()
    return pivot


def bake(
    out: Path,
    character: Character,
    animations: list[Animation],
    framing: Framing,
    settings: BakeSettings,
) -> None:
    directions = settings.direction_names

    setup_camera(framing)
    setup_lighting()
    setup_render(settings.size)
    pivot = parent_to_pivot()

    out.mkdir(parents=True, exist_ok=True)
    scene = bpy.context.scene
    total = 0

    for animation in animations:
        assign_action(character.armature, animation.action)
        start, end = animation.action.frame_range
        frames = sampled_frames(
            start,
            end,
            scene.render.fps,
            settings.fps[animation.name],
            settings.trim_start,
        )
        total += len(frames) * len(directions)

        for dir_index, dir_name in enumerate(directions):
            pivot.rotation_euler.z = direction_rotation(dir_index, len(directions))
            for frame_index, frame in enumerate(frames):
                scene.frame_set(frame)
                scene.render.filepath = str(
                    out / frame_filename(animation.name, dir_name, frame_index)
                )
                bpy.ops.render.render(write_still=True)
        print(
            f"baked {animation.name}: {len(directions)} dirs x {len(frames)} frames "
            f"(source action {animation.action.name!r})"
        )

    print(f"\ndone: {total} frames -> {out}")


def parse_rates(entries: list[str]) -> dict[str, int]:
    """`NAME=RATE` pairs into a mapping, one per animation."""
    rates = {}
    for entry in entries:
        name, _, rate = entry.partition("=")
        if not rate.isdigit():
            sys.exit(f"error: --fps needs NAME=RATE, got {entry!r}")
        rates[name] = int(rate)
    return rates


def load_animations(entries: list[str], character: Character) -> list[Animation]:
    animations = []
    for entry in entries:
        name, _, path = entry.partition("=")
        if not path:
            sys.exit(f"error: --animation needs NAME=PATH, got {entry!r}")
        action = take_action(Path(path), character.armature, name)
        animations.append(Animation(action=action, name=name))
    return animations


def main() -> None:
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    args = parse_args(argv)

    if args.inspect:
        if not args.glb:
            sys.exit("error: --inspect needs --glb PATH")
        clear_scene()
        inspect(import_glb(args.glb))
        return

    if not args.character:
        sys.exit("error: pass --character GLB with one or more --animation NAME=PATH")
    if not args.animation:
        sys.exit("error: --character needs at least one --animation NAME=PATH")
    settings = settings_from(args)

    clear_scene()
    character = import_glb(args.character)
    animations = load_animations(args.animation, character)
    missing = [a.name for a in animations if a.name not in settings.fps]
    if missing:
        sys.exit(f"error: no --fps given for {', '.join(missing)}")

    # Fix-ups must run after the actions are in, since both edit F-curves.
    apply_forearm_roll(character.armature, settings.forearm_roll)
    if not args.keep_root_motion:
        strip_root_motion(character.armature)

    # Everything is in one scene, so the camera is framed once across every
    # animation and the character cannot change size between them.
    framing = measure_framing(character, animations, settings)
    bake(args.out, character, animations, framing, settings)


if __name__ == "__main__":
    # Blender exits 0 on an uncaught exception, so a crashed bake would
    # otherwise be recorded as a success with a half-populated output dir.
    # SystemExit derives from BaseException, so the `sys.exit(...)` calls above
    # pass straight through with their own status.
    try:
        main()
    except Exception:  # noqa: BLE001
        traceback.print_exc()
        sys.exit(1)
