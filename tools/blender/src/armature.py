"""Carrying one armature in and out of a GLB.

`retarget_animation.py` fits a clip onto the canonical rig and
`promote_rig.py` makes that rig out of a character, and both end the same
way: one armature, nothing else, exported at its rest position. That shared
half lives here so neither script imports the other.

glTF has no standalone armature. Bones survive only as part of a skin, so an
armature exported alone is silently dropped and a one-triangle mesh weighted
to the root bone is what carries it through. The bake discards it on import.
"""

import pathlib

import bpy


def keep_only(armature: bpy.types.Object) -> None:
    """Empties the scene of everything but one armature.

    Every file this opens arrives with a body attached: the rig with its skin
    carrier, a provider's clip with a whole stock character, a rigged
    character with its mesh and texture. Only the skeleton is exported, so the
    rest goes.
    """
    for obj in list(bpy.data.objects):
        if obj is not armature:
            bpy.data.objects.remove(obj, do_unlink=True)
    for image in list(bpy.data.images):
        bpy.data.images.remove(image)
    for material in list(bpy.data.materials):
        bpy.data.materials.remove(material)


def skin_carrier(armature: bpy.types.Object) -> bpy.types.Object:
    """A tiny triangle weighted to the root bone, so the armature exports."""
    root = next(b for b in armature.data.bones if b.parent is None)
    mesh = bpy.data.meshes.new("skin_carrier")
    mesh.from_pydata(
        [(0.0, 0.0, 0.0), (0.001, 0.0, 0.0), (0.0, 0.001, 0.0)], [], [(0, 1, 2)]
    )
    mesh.update()

    carrier = bpy.data.objects.new("skin_carrier", mesh)
    bpy.context.collection.objects.link(carrier)
    group = carrier.vertex_groups.new(name=root.name)
    group.add([0, 1, 2], 1.0, "REPLACE")
    carrier.parent = armature
    modifier = carrier.modifiers.new("Armature", "ARMATURE")
    modifier.object = armature
    return carrier


def export(out: pathlib.Path, armature: bpy.types.Object) -> None:
    """Writes one armature, its skin carrier and whatever action it owns."""
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
        # `clip.twist` reads its rest term off the delivered joints, so the
        # armature must be exported at rest and not at the current frame.
        export_rest_position_armature=True,
    )
