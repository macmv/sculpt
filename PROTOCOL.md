# Sculpt Live mesh snapshot protocol (v1)

The body of each Unix-socket frame is a complete `MeshSnapshot`. All numeric
values are little-endian. The socket framing specified in `design.md` prepends
this body with a little-endian `u32` byte length.

| Offset | Type | Field |
| --- | --- | --- |
| 0 | `[u8; 4]` | Magic: ASCII `SCLP` |
| 4 | `u16` | Protocol version: `1` |
| 6 | `u16` | Header size: `132` |
| 8 | `[u8; 16]` | Stable source-object UUID, RFC 4122 byte order |
| 24 | `u64` | Monotonically increasing revision |
| 32 | `u32` | Flags (`1` means full snapshot) |
| 36 | `u32` | Vertex count |
| 40 | `u32` | Triangle count |
| 44 | `[f32; 16]` | Local-to-world transform, row-major |
| 108 | `[f32; 6]` | World dirty AABB: `min_x, min_y, min_z, max_x, max_y, max_z` |
| 132 | `[f32; vertex_count * 3]` | Local-space vertex positions, xyz order |
| … | `[u32; triangle_count * 3]` | Triangle vertex indices, three per triangle |
| … | `[u32; triangle_count]` | Blender material slot ID, one per triangle |

Manual publishing uses the whole evaluated object’s world bounds as its dirty
AABB. The Rust receiver must reject incorrect magic/version/header sizes,
overflowing buffer lengths, out-of-range indices, non-finite values, and stale
revisions before voxelization.
