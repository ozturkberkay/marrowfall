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
from pathlib import Path

import bpy
from actions import action_fcurves, assign_action, bone_basis, location_curves
from findings import Finding, guard, limits_from, write_report
from framing import (
    SAME_BODY,
    BakeSettings,
    Bounds,
    Camera,
    Framing,
    Landmark,
    Vec3,
    bone_from_data_path,
    direction_rotation,
    frame_filename,
    frames_are_keys,
    golden_samples,
    is_forearm,
    key_light_rotation,
    landmark_golden,
    missing_bones,
    off_this_body,
    pin_horizontally,
    project,
    rest_height,
    root_kept,
    root_travel,
    sampled_frames,
)
from mathutils import Vector
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
    source_height: float
    """Rest height of the rig this clip was authored on, sizing its lengths."""


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
        "removed, because the game moves the character and a traveling "
        "animation slides out of frame. Diagnostics only.",
    )
    parser.add_argument(
        "--limit",
        action="append",
        default=[],
        metavar="RULE=NUMBER",
        help="The published limit for one rule, passed by the runner.",
    )
    parser.add_argument(
        "--goldens",
        type=Path,
        help="Directory holding this character's landmark goldens, one file "
        "per clip per golden direction.",
    )
    parser.add_argument(
        "--golden-direction",
        action="append",
        default=[],
        metavar="NAME",
        help="A direction to take a landmark golden in. Repeatable, and the "
        "runner chooses which.",
    )
    parser.add_argument(
        "--update-goldens",
        action="store_true",
        help="Rewrite every golden from this run instead of reading it. The "
        "runner passes this only when MARROWFALL_UPDATE_GOLDENS is set.",
    )
    return parser.parse_args(argv)


def settings_from(args: argparse.Namespace) -> BakeSettings:
    """Validates the raw command line into the checked settings object."""
    return BakeSettings(
        directions=args.directions,
        fps=parse_rates(args.fps),
        size=args.size,
        trim_start=args.trim_start,
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
        for frame in frames_of(animation, settings):
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


def frames_of(animation: Animation, settings: BakeSettings) -> list[int]:
    """Which frames of one clip the bake renders.

    One owner, because the framing pass, the render and the two measurements
    must all be about the same frames.
    """
    start, end = animation.action.frame_range
    return sampled_frames(
        start,
        end,
        bpy.context.scene.render.fps,
        settings.fps[animation.name],
        settings.trim_start,
    )


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

    # Ambient fill via world color, lifts shadows so detail stays legible
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


def rest_points(armature: bpy.types.Object) -> list[Vec3]:
    """Rest bone heads, in the armature's own units.

    Not world space, deliberately: a pose bone's `location` sits *under* the
    armature object's transform, so folding that transform in here would count
    it twice on a rig whose object scale differs.

    Heads only. glTF stores no bone lengths, so the importer invents tails, and
    on this project's rigs they land tens of meters from the body.
    """
    return [
        (bone.head_local.x, bone.head_local.y, bone.head_local.z)
        for bone in armature.data.bones
    ]


def refuse_another_body(character: Character, animations: list[Animation]) -> None:
    """Refuses a clip authored against a rig this character's size is not.

    The retarget already sized every length by the femur and `clip.stride`
    measured that it did, so a clip reaching the bake is on this body and this
    reads 0. Scaling here would size it a second time.
    """
    height = rest_height(rest_points(character.armature))
    for animation in animations:
        try:
            off = off_this_body(animation.source_height, height)
        except ValueError as error:
            sys.exit(f"error: measuring {animation.name}: {error}")
        if off > SAME_BODY:
            sys.exit(
                f"error: {animation.name} was authored on a rig {off:.2e} off "
                f"this character's size, {animation.source_height:.4f} against "
                f"{height:.4f} in armature units. It reached the bake unfitted"
            )


def root_bones(armature: bpy.types.Object) -> list[str]:
    """Every bone with nothing above it, which is what carries travel."""
    return [bone.name for bone in armature.pose.bones if bone.parent is None]


def strip_root_motion(armature: bpy.types.Object) -> None:
    """Pins the root bone's horizontal WORLD motion across every action.

    The game moves the character itself, so a traveling clip would slide out
    of frame and inflate the crop. In world space, because a root bone's own
    channels are not world axes: `Hips` local Z is world minus Z tilted 8.9
    degrees on this rig. Height is kept, because a bob is animation. The
    horizontal is held at the first frame rather than zeroed, so the character
    keeps whatever offset the rig was authored with.
    """
    roots = root_bones(armature)
    for action in bpy.data.actions:
        for name in roots:
            curves = location_curves(action, name)
            if not curves:
                continue
            basis = bone_basis(armature, name)
            inverse = basis.inverted()
            keys = range(len(curves[0].keyframe_points))
            world = [
                basis @ Vector([curve.keyframe_points[key].co[1] for curve in curves])
                for key in keys
            ]
            pinned = pin_horizontally([tuple(point) for point in world])
            for key, point in zip(keys, pinned, strict=True):
                for curve, value in zip(curves, inverse @ Vector(point), strict=True):
                    written = curve.keyframe_points[key]
                    written.co[1] = value
                    written.handle_left[1] = value
                    written.handle_right[1] = value
            for curve in curves:
                curve.update()
    print(f"pinned root motion on {roots}")


def measure_root_travel(
    armature: bpy.types.Object,
    animations: list[Animation],
    stripped: bool,
    limits: dict[str, float],
) -> list[Finding]:
    """`clip.root_travel` and `clip.root_bob`, per axis per clip, on the copy
    just pinned.

    The pinned copy is never written to disk, so this is the only boundary
    where the residual can be read at all.
    """
    scene = bpy.context.scene
    findings = []
    for animation in animations:
        assign_action(armature, animation.action)
        start, end = animation.action.frame_range
        for bone in root_bones(armature):
            if not stripped:
                findings += root_kept(animation.name, bone, limits)
                continue
            path = []
            for frame in range(round(start), round(end) + 1):
                scene.frame_set(frame)
                head = armature.matrix_world @ armature.pose.bones[bone].matrix
                path.append(tuple(head.to_translation()))
            findings += root_travel(animation.name, bone, path, limits)
    return findings


def take_action(
    path: Path, target: bpy.types.Object, name: str
) -> tuple[bpy.types.Action, float]:
    """Loads one action out of `path`, with the rest height it was built for.

    Hands the action to `target`'s armature.

    Animation-only files still carry an armature, because glTF animations target
    nodes inside their own file, the format has no cross-file reference. The
    imported armature is therefore thrown away after its action has been taken;
    bone names match, so the action drives the character's own skeleton.

    Nothing here compares the two bind poses. The retarget transfers motion in
    world space against the aim table, so a clip fitted to this skeleton is
    already in this rest pose, and the old check read the importer's invented
    bone tails to say so.
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
            f"error: {path} has no armature, so the body it was authored for "
            "cannot be measured. Re-download it."
        )
    source_height = rest_height(rest_points(source_rig))
    # Discard the imported skeleton; only its measurements were wanted.
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
    return action, source_height


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


def setup_scene(framing: Framing, settings: BakeSettings) -> bpy.types.Object:
    """The camera, the lights, the render settings and the pivot the character
    turns on.

    Separate from the render so the goldens are projected through the scene the
    frames are drawn in, and measured before eight minutes of rendering rather
    than after.
    """
    setup_camera(framing)
    setup_lighting()
    setup_render(settings.size)
    return parent_to_pivot()


def bake(
    out: Path,
    character: Character,
    animations: list[Animation],
    settings: BakeSettings,
    pivot: bpy.types.Object,
) -> None:
    directions = settings.direction_names
    out.mkdir(parents=True, exist_ok=True)
    scene = bpy.context.scene
    total = 0

    for animation in animations:
        assign_action(character.armature, animation.action)
        frames = frames_of(animation, settings)
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


def measure_sampled_frames(
    animations: list[Animation], settings: BakeSettings, limits: dict[str, float]
) -> list[Finding]:
    """`bake.sampled_frames_are_keys`, one finding per clip.

    Read off the action rather than the file, which is the only place the keys
    behind a rendered pose can be seen at all.
    """
    return [
        frames_are_keys(
            animation.name,
            frames_of(animation, settings),
            [
                [point.co[0] for point in curve.keyframe_points]
                for curve in action_fcurves(animation.action)
            ],
            limits,
        )
        for animation in animations
    ]


def bake_camera(settings: BakeSettings) -> Camera:
    """The scene's own camera, as the pixel mapper a golden is projected with."""
    # Nothing has evaluated the scene since the camera was placed, and a
    # transform set through the API reaches `matrix_world` only when it does.
    bpy.context.view_layer.update()
    camera = bpy.context.scene.camera
    # A camera carries no scale, so its own axes are where it turned them: +X
    # is right across the image and +Y is up it.
    turned = camera.matrix_world.to_quaternion()
    return Camera(
        location=tuple(camera.matrix_world.translation),
        right=tuple(turned @ Vector((1.0, 0.0, 0.0))),
        up=tuple(turned @ Vector((0.0, 1.0, 0.0))),
        ortho_scale=camera.data.ortho_scale,
        size=settings.size,
    )


def joint_pixels(
    character: Character, frame: int, camera: Camera, bones: list[str]
) -> list[Landmark]:
    """Every joint of one pose, in the pixels of the frame that renders it."""
    armature = character.armature
    marks = []
    for bone in bones:
        world = armature.matrix_world @ armature.pose.bones[bone].matrix
        x, y = project(tuple(world.to_translation()), camera)
        marks.append(Landmark(frame=frame, bone=bone, x=x, y=y))
    return marks


def measure_goldens(
    character: Character,
    animations: list[Animation],
    settings: BakeSettings,
    pivot: bpy.types.Object,
    goldens: Path | None,
    directions: list[str],
    update: bool,
    limits: dict[str, float],
) -> list[Finding]:
    """`bake.landmark_golden`, three frames by two directions per clip.

    The pivot is turned to each golden direction first, so what is projected
    is where the joints are in the frame that direction renders.
    """
    if goldens is None:
        sys.exit("error: --goldens DIR is needed to read the landmark goldens")
    scene = bpy.context.scene
    camera = bake_camera(settings)
    ring = settings.direction_names
    bones = sorted(bone.name for bone in character.armature.data.bones)
    findings = []
    for animation in animations:
        assign_action(character.armature, animation.action)
        frames = frames_of(animation, settings)
        for direction in directions:
            if direction not in ring:
                sys.exit(f"error: --golden-direction {direction} is not in {ring}")
            pivot.rotation_euler.z = direction_rotation(
                ring.index(direction), len(ring)
            )
            measured = []
            for sample in golden_samples(len(frames)):
                scene.frame_set(frames[sample])
                measured += joint_pixels(character, sample, camera, bones)
            subject = f"{animation.name}_{direction}"
            findings.append(
                landmark_golden(
                    subject, goldens / f"{subject}.txt", measured, limits, update
                )
            )
    return findings


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
        action, source_height = take_action(Path(path), character.armature, name)
        animations.append(
            Animation(action=action, name=name, source_height=source_height)
        )
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

    refuse_another_body(character, animations)
    stripped = not args.keep_root_motion
    if stripped:
        strip_root_motion(character.armature)
    limits = limits_from(args.limit)
    findings = measure_root_travel(character.armature, animations, stripped, limits)
    findings += measure_sampled_frames(animations, settings, limits)

    # Everything is in one scene, so the camera is framed once across every
    # animation and the character cannot change size between them.
    framing = measure_framing(character, animations, settings)
    pivot = setup_scene(framing, settings)
    findings += measure_goldens(
        character,
        animations,
        settings,
        pivot,
        args.goldens,
        args.golden_direction,
        args.update_goldens,
        limits,
    )
    # The report is the deliverable; the render is two minutes. A moved
    # golden or an unkeyed frame stops here, so it costs seconds.
    if write_report(findings).has_errors:
        return
    bake(args.out, character, animations, settings, pivot)


def save_blend(path: Path) -> None:
    """The scene as it stood when the bake failed, for a human to open."""
    bpy.ops.wm.save_as_mainfile(filepath=str(path), copy=True)


if __name__ == "__main__":
    # Blender exits 0 on an uncaught exception, so a crashed bake would
    # otherwise be recorded as a success with a half-populated output dir.
    # `guard` writes the success sentinel the Rust side asserts.
    guard(main, save_blend)
