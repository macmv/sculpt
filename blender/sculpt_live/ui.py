"""3D View UI for Sculpt Live."""

import bpy


class CLIVE_UL_materials(bpy.types.UIList):
    """Compact color-to-block palette rows."""

    def draw_item(self, _context, layout, _data, item, _icon, _active_data, _active_property, _index):
        row = layout.row(align=True)
        row.prop(item, "color", text="")
        row.prop(item, "block_state", text="")


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
        layout.prop(settings, "socket_path")
        mapping = layout.box()
        mapping.label(text="Minecraft Coordinates")
        mapping.prop(settings, "blender_units_per_block")
        mapping.prop(settings, "minecraft_origin")
        mapping.label(text="X = X, Y = Z, Z = -Y", icon="INFO")
        palette = layout.box()
        palette.label(text="Painted Block Colors")
        palette.label(text="Sculpt Paint uses the active color attribute", icon="INFO")
        row = palette.row()
        row.template_list(
            "CLIVE_UL_materials", "sculpt_live_materials", settings, "materials", settings, "active_material", rows=3
        )
        buttons = row.column(align=True)
        buttons.operator("sculpt_live.add_material", text="", icon="ADD")
        buttons.operator("sculpt_live.remove_material", text="", icon="REMOVE")
        if settings.materials:
            material = settings.materials[min(settings.active_material, len(settings.materials) - 1)]
            palette.prop(material, "color")
            palette.prop(material, "block_state")
        layout.separator()
        layout.operator("sculpt_live.publish_snapshot", icon="EXPORT")
        layout.label(text=f"Last revision: {settings.revision}")
        if settings.last_snapshot_bytes:
            layout.label(text=f"Snapshot size: {settings.last_snapshot_bytes:,} bytes")
