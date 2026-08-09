"""Operators for extracting evaluated mesh snapshots.

Network transport belongs behind this boundary. Dyntopo can change topology on
every stroke, so publishing always begins from a complete evaluated mesh.
"""

import bpy

from .protocol import build_mesh_snapshot
from .transport import send_snapshot


class CLIVE_OT_use_material_brush(bpy.types.Operator):
    """Set Sculpt Paint's brush color from this palette entry"""

    bl_idname = "sculpt_live.use_material_brush"
    bl_label = "Use Paint Color"

    index: bpy.props.IntProperty(min=0)

    def execute(self, context):
        materials = context.scene.sculpt_live.materials
        if self.index >= len(materials):
            return {"CANCELLED"}
        color = materials[self.index].color[:3]
        brush = context.tool_settings.sculpt.brush
        if brush is None:
            self.report({"ERROR"}, "Choose a Sculpt Paint brush first")
            return {"CANCELLED"}
        brush.color = color
        sculpt_settings = context.tool_settings.sculpt
        sculpt_settings.unified_paint_settings.color = color
        sculpt_settings.unified_paint_settings.use_unified_color = True
        # Sculpt Paint uses the same color controls as Vertex Paint.  Set its
        # brush too when Blender exposes one, so either paint tool stays in
        # sync with the palette selection.
        vertex_brush = context.tool_settings.vertex_paint.brush
        if vertex_brush is not None:
            vertex_brush.color = color
            vertex_settings = context.tool_settings.vertex_paint
            vertex_settings.unified_paint_settings.color = color
            vertex_settings.unified_paint_settings.use_unified_color = True
        return {"FINISHED"}


class CLIVE_OT_move_material(bpy.types.Operator):
    """Move the selected palette entry"""

    bl_idname = "sculpt_live.move_material"
    bl_label = "Move Block Color"

    direction: bpy.props.EnumProperty(items=(("UP", "Up", ""), ("DOWN", "Down", "")))

    def execute(self, context):
        settings = context.scene.sculpt_live
        index = settings.active_material
        target = index - 1 if self.direction == "UP" else index + 1
        if 0 <= target < len(settings.materials):
            settings.materials.move(index, target)
            settings.active_material = target
        return {"FINISHED"}


class CLIVE_OT_add_material(bpy.types.Operator):
    """Add a painted-color to Minecraft-block mapping"""

    bl_idname = "sculpt_live.add_material"
    bl_label = "Add Block Color"

    def execute(self, context):
        settings = context.scene.sculpt_live
        settings.materials.add()
        settings.active_material = len(settings.materials) - 1
        return {"FINISHED"}


class CLIVE_OT_add_feature(bpy.types.Operator):
    bl_idname = "sculpt_live.add_feature"
    bl_label = "Add Surface Feature"

    def execute(self, context):
        settings = context.scene.sculpt_live
        if not settings.materials:
            return {"CANCELLED"}
        material = settings.materials[min(settings.active_material, len(settings.materials) - 1)]
        material.features.add()
        material.active_feature = len(material.features) - 1
        return {"FINISHED"}


class CLIVE_OT_remove_feature(bpy.types.Operator):
    bl_idname = "sculpt_live.remove_feature"
    bl_label = "Remove Surface Feature"

    def execute(self, context):
        settings = context.scene.sculpt_live
        if not settings.materials:
            return {"CANCELLED"}
        material = settings.materials[min(settings.active_material, len(settings.materials) - 1)]
        if material.features:
            material.features.remove(min(material.active_feature, len(material.features) - 1))
            material.active_feature = max(0, min(material.active_feature, len(material.features) - 1))
        return {"FINISHED"}


class CLIVE_OT_remove_material(bpy.types.Operator):
    """Remove the selected painted-color mapping"""

    bl_idname = "sculpt_live.remove_material"
    bl_label = "Remove Block Color"

    def execute(self, context):
        settings = context.scene.sculpt_live
        if settings.materials:
            settings.materials.remove(min(settings.active_material, len(settings.materials) - 1))
            settings.active_material = max(0, min(settings.active_material, len(settings.materials) - 1))
        return {"FINISHED"}


class CLIVE_OT_publish_snapshot(bpy.types.Operator):
    """Send a full evaluated-mesh snapshot to the voxel service"""

    bl_idname = "sculpt_live.publish_snapshot"
    bl_label = "Publish Snapshot"
    bl_options = {"REGISTER"}

    @classmethod
    def poll(cls, context):
        settings = context.scene.sculpt_live
        return settings.source_object is not None and settings.source_object.type == "MESH"

    def execute(self, context):
        settings = context.scene.sculpt_live
        source = settings.source_object
        depsgraph = context.evaluated_depsgraph_get()
        evaluated = source.evaluated_get(depsgraph)
        mesh = evaluated.to_mesh()

        try:
            if not mesh.polygons:
                self.report({"ERROR"}, "The sculpt object has no faces to publish")
                return {"CANCELLED"}

            next_revision = settings.revision + 1
            snapshot = build_mesh_snapshot(source, evaluated, mesh, next_revision, settings)
            send_snapshot(settings.socket_path, snapshot.payload)
            settings.revision = next_revision
            settings.last_snapshot_bytes = len(snapshot.payload)
            self.report(
                {"INFO"},
                f"Sent revision {settings.revision}: "
                f"{snapshot.vertex_count} vertices, {snapshot.triangle_count} triangles, "
                f"{settings.last_snapshot_bytes:,} bytes",
            )
        except (OSError, ValueError) as error:
            self.report({"ERROR"}, f"Publish failed: {error}")
            return {"CANCELLED"}
        finally:
            evaluated.to_mesh_clear()

        return {"FINISHED"}
