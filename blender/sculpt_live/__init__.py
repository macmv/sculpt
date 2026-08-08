"""Blender entry point for Sculpt Live."""

bl_info = {
    "name": "Sculpt Live",
    "author": "Sculpt Live contributors",
    "version": (0, 1, 0),
    "blender": (4, 2, 0),
    "location": "View3D > Sidebar > Sculpt Live",
    "description": "Publish Blender sculpt snapshots to a local voxel service",
    "category": "3D View",
}

from .operators import CLIVE_OT_publish_snapshot
from .properties import CLIVE_PG_settings
from .ui import CLIVE_PT_panel


CLASSES = (
    CLIVE_PG_settings,
    CLIVE_OT_publish_snapshot,
    CLIVE_PT_panel,
)


def register():
    import bpy

    for cls in CLASSES:
        bpy.utils.register_class(cls)
    bpy.types.Scene.sculpt_live = bpy.props.PointerProperty(type=CLIVE_PG_settings)


def unregister():
    import bpy

    del bpy.types.Scene.sculpt_live
    for cls in reversed(CLASSES):
        bpy.utils.unregister_class(cls)
