"""3D View UI for Sculpt Live."""

import bpy


class CLIVE_PT_panel(bpy.types.Panel):
    bl_label = "Sculpt Live"
    bl_idname = "CLIVE_PT_sculpt_live"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "Sculpt Live"

    def draw(self, context):
        layout = self.layout
        settings = context.scene.sculpt_live

        layout.prop(settings, "source_object")
        layout.prop(settings, "service_address")
        layout.separator()
        layout.operator("sculpt_live.publish_snapshot", icon="EXPORT")
        layout.label(text=f"Last revision: {settings.revision}")
