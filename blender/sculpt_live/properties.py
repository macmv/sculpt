"""Scene-level settings owned by the Sculpt Live add-on."""

import bpy


class CLIVE_PG_settings(bpy.types.PropertyGroup):
    source_object: bpy.props.PointerProperty(
        name="Sculpt Object",
        description="Mesh object to evaluate and publish",
        type=bpy.types.Object,
        poll=lambda _self, obj: obj.type == "MESH",
    )
    service_address: bpy.props.StringProperty(
        name="Service Address",
        description="Local Rust voxel service endpoint",
        default="ws://127.0.0.1:8765",
    )
    revision: bpy.props.IntProperty(
        name="Revision",
        description="Last revision reserved for a published mesh snapshot",
        default=0,
        min=0,
    )
