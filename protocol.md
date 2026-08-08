# Sculpt Live mesh snapshot protocol

The body of each Unix-socket frame is a complete `MeshSnapshot`. All numeric
values are little-endian. The socket framing prepends this body with a
little-endian `u32` byte length.

| Offset | Type | Field |
| --- | --- | --- |
| 0 | `[u8; 4]` | Magic: ASCII `SCLP` |
| 4 | `u16` | Header size: `146` |
| 6 | `[u8; 16]` | Stable source-object UUID |
| 22 | `u64` | Monotonically increasing revision |
| 30 | `u32` | Flags (`1` means full snapshot) |
| 34 | `u32` | Vertex count |
| 38 | `u32` | Triangle count |
| 42 | `[f32; 16]` | Local-to-world transform, row-major |
| 106 | `[f32; 6]` | World dirty AABB: min xyz, then max xyz |
| 130 | `f32` | Blender world units per Minecraft block |
| 134 | `[i32; 3]` | Minecraft block coordinate at Blender world origin |
| 146 | `[f32; vertex_count * 3]` | Local-space vertex positions, xyz order |
| … | `[u32; triangle_count * 3]` | Triangle vertex indices |
| … | `[u32; triangle_count]` | Blender material slot ID per triangle |

Convert a Blender world point `(x, y, z)` to Minecraft block-space with
`(origin_x + x / units, origin_y + z / units, origin_z - y / units)`. The
Rust receiver must validate sizes, finite values, indices, and revisions before
voxelizing.
