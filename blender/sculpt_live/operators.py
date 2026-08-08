"""Operators for extracting evaluated mesh snapshots.

Network transport belongs behind this boundary. Dyntopo can change topology on
every stroke, so publishing always begins from a complete evaluated mesh.
"""

import bpy

from .protocol import build_mesh_snapshot
from .transport import send_snapshot


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
