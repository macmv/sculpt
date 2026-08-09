"""3D View UI for Sculpt Live."""

import bpy


class CLIVE_UL_materials(bpy.types.UIList):
    """Compact painted-material palette rows."""

    def draw_item(self, _context, layout, _data, item, _icon, _active_data, _active_property, _index):
        row = layout.row(align=True)
        swatch = row.row(align=True)
        swatch.ui_units_x = 1.5
        swatch.prop(item, "color", text="")
        use_brush = row.operator("sculpt_live.use_material_brush", text="", icon="BRUSH_DATA")
        use_brush.index = _index


class CLIVE_UL_features(bpy.types.UIList):
    """Readable summaries for a material's surface-feature list."""

    def draw_item(self, _context, layout, _data, item, _icon, _active_data, _active_property, _index):
        if item.kind == "SCATTER":
            summary = f"Scatter: {item.scatter_block} (every {item.interval})"
        else:
            summary = (
                f"Tree: {item.trunk_block} + {item.leaves_block} "
                f"({item.tree_height} high, radius {item.canopy_radius}, every {item.interval})"
            )
        layout.label(text=summary, icon="PARTICLES")


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
        palette.label(text="Painted Materials")
        palette.label(text="Sculpt Paint uses the active color attribute", icon="INFO")
        row = palette.row()
        row.template_list(
            "CLIVE_UL_materials", "sculpt_live_materials", settings, "materials", settings, "active_material", rows=3
        )
        buttons = row.column(align=True)
        buttons.operator("sculpt_live.add_material", text="", icon="ADD")
        buttons.operator("sculpt_live.remove_material", text="", icon="REMOVE")
        if len(settings.materials) > 1:
            buttons.separator()
            move_up = buttons.operator("sculpt_live.move_material", text="", icon="TRIA_UP")
            move_up.direction = "UP"
            move_down = buttons.operator("sculpt_live.move_material", text="", icon="TRIA_DOWN")
            move_down.direction = "DOWN"
        if settings.materials:
            material = settings.materials[min(settings.active_material, len(settings.materials) - 1)]
            palette.prop(material, "base_block")
            palette.prop(material, "underground_block")
            palette.prop(material, "base_depth")
            features = palette.box()
            features.label(text="Surface Features")
            row = features.row()
            row.template_list("CLIVE_UL_features", "sculpt_live_features", material, "features", material, "active_feature", rows=2)
            buttons = row.column(align=True)
            buttons.operator("sculpt_live.add_feature", text="", icon="ADD")
            buttons.operator("sculpt_live.remove_feature", text="", icon="REMOVE")
            if material.features:
                feature = material.features[min(material.active_feature, len(material.features) - 1)]
                features.prop(feature, "kind")
                features.prop(feature, "interval")
                if feature.kind == "SCATTER":
                    features.prop(feature, "scatter_block")
                else:
                    features.prop(feature, "trunk_block")
                    features.prop(feature, "leaves_block")
                    features.prop(feature, "tree_height")
                    features.prop(feature, "canopy_radius")
        layout.separator()
        layout.operator("sculpt_live.publish_snapshot", icon="EXPORT")
        layout.label(text=f"Last revision: {settings.revision}")
        if settings.last_snapshot_bytes:
            layout.label(text=f"Snapshot size: {settings.last_snapshot_bytes:,} bytes")
