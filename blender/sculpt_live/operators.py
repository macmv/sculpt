"""Operators for extracting evaluated mesh snapshots.

Network transport belongs behind this boundary. Dyntopo can change topology on
every stroke, so publishing always begins from a complete evaluated mesh.
"""

import bpy


class CLIVE_OT_publish_snapshot(bpy.types.Operator):
    """Prepare a full evaluated-mesh snapshot for the voxel service"""

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

            # This is intentionally the point where a binary MeshSnapshot is
            # assembled: float32 world-space vertices, uint32 triangle indices,
            # material IDs, transform, revision, and dirty-AABB hint.
            vertex_count = len(mesh.vertices)
            triangle_count = sum(len(polygon.vertices) - 2 for polygon in mesh.polygons)
            settings.revision += 1
            self.report(
                {"INFO"},
                f"Prepared revision {settings.revision}: "
                f"{vertex_count} vertices, {triangle_count} triangles",
            )
        finally:
            evaluated.to_mesh_clear()

        return {"FINISHED"}
