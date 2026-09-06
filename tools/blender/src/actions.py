"""The F-curve edits both the retarget and the bake make on a rig's action.

`bake_sprites.py` sizes nothing and lifts nothing, and `retarget_animation.py`
renders nothing, but both bind an action to an armature and both rewrite
`location` keys. That shared half lives here so neither script imports the
other: a helper with its only caller in a different file is one nobody knows
who owns.

Needs `bpy`, so the maths it serves stays in `framing.py` and `clip.py`, which
do not. Two things are deliberate.

- **No object transform is ever applied**, only composed. Applying one to a rig
  that owns an action rescales the rest geometry and leaves every location key
  byte identical, so 2.316 m of travel silently becomes 231.599 m.
- **Every edit writes the handles too.** A keyframe carries `co` plus two
  Bezier handles, and a value written to `co` alone is dragged back by them.
"""

import sys

import bpy
from framing import root_channel_fault
from mathutils import Matrix


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


def fcurve_owners(action: bpy.types.Action) -> list:
    """Every collection of F-curves in an action, across Blender's two APIs.

    Blender 4.4 introduced slotted actions, where curves live under
    layers/strips/channelbags; older files expose `action.fcurves` directly.
    Removing a curve needs the collection holding it, which is why this is the
    primitive and `action_fcurves` is built on it.
    """
    if hasattr(action, "fcurves"):
        return [action]
    return [
        channelbag
        for layer in action.layers
        for strip in layer.strips
        for channelbag in getattr(strip, "channelbags", ())
    ]


def action_fcurves(action: bpy.types.Action) -> list[bpy.types.FCurve]:
    """Every F-curve in an action."""
    return [curve for owner in fcurve_owners(action) for curve in owner.fcurves]


def location_curves(action: bpy.types.Action, bone: str) -> list[bpy.types.FCurve]:
    """A bone's three location curves in x, y, z order, or none at all.

    Anything in between is refused rather than skipped: a bone silently left
    out of the strip is one that keeps traveling.
    """
    path = f'pose.bones["{bone}"].location'
    curves = sorted(
        (fc for fc in action_fcurves(action) if fc.data_path == path),
        key=lambda fc: fc.array_index,
    )
    fault = root_channel_fault(
        bone, action.name, [len(fc.keyframe_points) for fc in curves]
    )
    if fault is not None:
        sys.exit(f"error: {fault}")
    return curves


def scale_translation(action: bpy.types.Action, ratio: float) -> None:
    """Sizes every `location` curve in an action.

    Every curve, not only the three that are non-zero today: a constant offset
    is a length too, and it is wrong on a differently sized body.
    """
    for curve in action_fcurves(action):
        if not curve.data_path.endswith(".location"):
            continue
        for point in curve.keyframe_points:
            point.co[1] *= ratio
            point.handle_left[1] *= ratio
            point.handle_right[1] *= ratio
        curve.update()


def bone_basis(armature: bpy.types.Object, bone: str) -> Matrix:
    """What turns a root bone's `location` into a world displacement.

    Composed, never applied: `matrix_world @ matrix_local` is the frame the
    bone's own `location` is expressed in, and a root bone has no parent to
    compose above it.
    """
    return (armature.matrix_world @ armature.data.bones[bone].matrix_local).to_3x3()
