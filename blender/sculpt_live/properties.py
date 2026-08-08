"""Scene-level settings owned by the Sculpt Live add-on."""

import bpy


class CLIVE_PG_settings(bpy.types.PropertyGroup):
    source_object: bpy.props.PointerProperty(
        name="Sculpt Object",
        description="Mesh object to evaluate and publish",
        type=bpy.types.Object,
        poll=lambda _self, obj: obj.type == "MESH",
    )
    socket_path: bpy.props.StringProperty(
        name="Socket Path",
        description="Unix domain socket owned by the local Rust voxel service",
        default="/tmp/sculpt-live.sock",
    )
    blender_units_per_block: bpy.props.FloatProperty(
        name="Blender Units per Block",
        description="Blender world-space distance represented by one Minecraft block",
        default=1.0,
        min=0.0001,
    )
    minecraft_origin: bpy.props.IntVectorProperty(
        name="Minecraft Origin",
        description="Minecraft block coordinate corresponding to Blender world origin",
        size=3,
        default=(0, 0, 0),
    )
    revision: bpy.props.IntProperty(
        name="Revision",
        description="Last revision reserved for a published mesh snapshot",
        default=0,
        min=0,
    )
    last_snapshot_bytes: bpy.props.IntProperty(
        name="Snapshot Size",
        description="Bytes in the most recently prepared binary mesh snapshot",
        default=0,
        min=0,
    )
