"""Blender entry point for Sculpt Live."""

bl_info = {
    "name": "Sculpt Live",
    "author": "Sculpt Live contributors",
    "version": (0, 3, 0),
    "blender": (4, 2, 0),
    "location": "View3D > Sidebar > Sculpt Live",
    "description": "Publish Blender sculpt snapshots to a local voxel service",
    "category": "3D View",
}

from .operators import (
    CLIVE_OT_add_material,
    CLIVE_OT_add_feature,
    CLIVE_OT_move_material,
    CLIVE_OT_publish_snapshot,
    CLIVE_OT_remove_material,
    CLIVE_OT_remove_feature,
    CLIVE_OT_use_material_brush,
    publish_scene_snapshot,
)
from .properties import CLIVE_PG_feature, CLIVE_PG_material, CLIVE_PG_settings
from .ui import CLIVE_PT_panel, CLIVE_UL_features, CLIVE_UL_materials
from bpy.app.handlers import persistent


CLASSES = (
    CLIVE_PG_feature,
    CLIVE_PG_material,
    CLIVE_PG_settings,
    CLIVE_OT_add_material,
    CLIVE_OT_remove_material,
    CLIVE_OT_add_feature,
    CLIVE_OT_remove_feature,
    CLIVE_OT_use_material_brush,
    CLIVE_OT_move_material,
    CLIVE_OT_publish_snapshot,
    CLIVE_UL_materials,
    CLIVE_UL_features,
    CLIVE_PT_panel,
)


@persistent
def _publish_snapshot_on_save(_unused):
    """Send the active scene's configured sculpt after a successful file save."""
    import bpy

    scene = bpy.context.scene
    if scene is None or not hasattr(scene, "sculpt_live"):
        return

    try:
        snapshot = publish_scene_snapshot(scene, bpy.context.evaluated_depsgraph_get())
        print(
            "Sculpt Live: sent snapshot on save "
            f"(revision {scene.sculpt_live.revision}, "
            f"{snapshot.vertex_count} vertices, {snapshot.triangle_count} triangles)"
        )
    except Exception as error:
        # A failed local publish must not turn a successful .blend save into an
        # error, including when Blender cannot evaluate the configured mesh.
        # The message is visible in Blender's system console.
        print(f"Sculpt Live: snapshot on save failed: {error}")


def register():
    import bpy

    for cls in CLASSES:
        bpy.utils.register_class(cls)
    bpy.types.Scene.sculpt_live = bpy.props.PointerProperty(type=CLIVE_PG_settings)
    if _publish_snapshot_on_save not in bpy.app.handlers.save_post:
        bpy.app.handlers.save_post.append(_publish_snapshot_on_save)


def unregister():
    import bpy

    if _publish_snapshot_on_save in bpy.app.handlers.save_post:
        bpy.app.handlers.save_post.remove(_publish_snapshot_on_save)
    del bpy.types.Scene.sculpt_live
    for cls in reversed(CLASSES):
        bpy.utils.unregister_class(cls)
