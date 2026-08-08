# Sculpt Live Protocol

This is the local, Unix-domain-socket protocol between Blender, the Rust core,
and Minecraft. The core is the only listener, at `/tmp/sculpt-live.sock`.
Every peer connects to it independently. All multi-byte numeric fields are
little-endian; strings are UTF-8; no message uses JSON.

## Framing and connection roles

Each stream message is `u32 payload_length` followed by exactly that many
payload bytes. A peer must send its role-identifying first payload immediately
after connecting:

| Peer | First packet | Direction |
| --- | --- | --- |
| Blender publisher | `SCLP` mesh snapshot | Blender -> core |
| Minecraft subscriber | `SCLM` hello | Minecraft -> core |

`payload_length` must be non-zero and at most 4 MiB. Core closes the connection
for malformed framing, an unknown first packet, invalid
UTF-8, or a packet whose declared sizes do not exactly consume its payload.
Messages after the first are legal only for the role assigned by that first
packet. This is an intentionally unversioned, local development protocol; a
breaking change updates this document and both local peers together. Core must make all subscriber writes through bounded per-subscriber
queues; it must never wait for a subscriber while ingesting or voxelizing a
mesh. A newer delta replaces a queued older delta for the same section
coordinates.

## Blender publisher to core: `SCLP`

`SCLP` remains the existing mesh-snapshot packet so current Blender transport
is compatible. Its body is:

| Offset | Type | Field |
| --- | --- | --- |
| 0 | `[u8; 4]` | ASCII `SCLP` |
| 4 | `u16` | Header size, currently `130` |
| 6 | `u64` | Global monotonically increasing revision |
| 14 | `u32` | Flags (`1` = complete snapshot; other bits zero) |
| 18 | `u32` | Vertex count |
| 22 | `u32` | Triangle count |
| 26 | `[f32; 16]` | Local-to-world transform, row-major |
| 90 | `[f32; 6]` | World dirty AABB: min xyz, then max xyz |
| 114 | `f32` | Blender world units per Minecraft block |
| 118 | `[i32; 3]` | Minecraft block coordinate at Blender world origin |
| 130 | `[f32; vertex_count * 3]` | Local-space vertex positions, xyz |
| … | `[u32; triangle_count * 3]` | Triangle vertex indices |
| … | `[u32; triangle_count]` | Blender material slot per triangle |

The core validates finite values, dimensions, index bounds, and revision before
accepting the snapshot. A Blender world point `(x, y, z)` maps
to `(origin_x + x / units, origin_y + z / units, origin_z - y / units)` in
Minecraft block coordinates.

## Minecraft control packets

### `SCLM` — subscribe/hello (Minecraft -> core)

| Offset | Type | Field |
| --- | --- | --- |
| 0 | `[u8; 4]` | ASCII `SCLM` |
| 4 | `u16` | State count, 1–65535 |
| … | repeated | `u16` UTF-8 byte length + canonical block-state string |

The registry is the complete set of canonical Overworld block-state strings
that this Minecraft instance can resolve. It must contain `minecraft:air`, may
not contain an empty or duplicate string, and must consume the frame exactly.
Core validates every state it puts into an `SCLD` palette against this set.
The registry is connection-local and remains valid until that subscriber
disconnects; a changed registry requires a new connection.

### `SCLW` — welcome (core -> Minecraft)

| Offset | Type | Field |
| --- | --- | --- |
| 0 | `[u8; 4]` | ASCII `SCLW` |
| 4 | `u32` | Core capability flags; currently zero |

### `SCLA` — delta disposition (Minecraft -> core)

| Offset | Type | Field |
| --- | --- | --- |
| 0 | `[u8; 4]` | ASCII `SCLA` |
| 4 | `u64` | Revision |
| 12 | `[i32; 3]` | Section x, y, z |
| 24 | `u8` | Result: `0` installed, `1` superseded, `2` rejected |

Acknowledgements are advisory telemetry and must not be required for core to
continue. Minecraft may send `superseded` for work replaced before installation
and `rejected` for a state it cannot resolve. Core may use these to report
health, but does not retry a rejected delta indefinitely.

### `SCLE` — protocol error (either direction, then close)

| Offset | Type | Field |
| --- | --- | --- |
| 0 | `[u8; 4]` | ASCII `SCLE` |
| 4 | `u16` | Error code |
| 6 | `u16` | UTF-8 message byte length |
| 8 | `[u8; length]` | Short diagnostic, maximum 512 bytes |

## Core to Minecraft: `SCLD` complete section replacement

`SCLD` is the only terrain packet. It replaces—not patches—the
entire target 16×16×16 Overworld section. Core performs voxelization and
palette compression; Minecraft must not expand this into 4,096
`ServerWorld.setBlockState` calls.

| Offset | Type | Field |
| --- | --- | --- |
| 0 | `[u8; 4]` | ASCII `SCLD` |
| 4 | `u64` | Global revision |
| 12 | `[i32; 3]` | Minecraft section x, y, z |
| 24 | `u16` | Palette entry count, 1–4096 |
| … | repeated | `u16` UTF-8 byte length + block-state string |
| … | `u8` | Bits per palette index |
| … | `u16` | Packed `u64` word count |
| … | `[u64; word_count]` | Palette indices, least-significant-bit first |

A palette entry is a resolved registry block-state string from that
subscriber's accepted `SCLM` registry, for example
`minecraft:stone` or `minecraft:oak_log[axis=y]`; it contains no numeric
runtime block IDs. Minecraft resolves it against its local Overworld registry.
The cell order is `x + 16 * (z + 16 * y)`, where `x`, `y`, and `z` are local
coordinates 0–15. For a palette size of one, `bits` and `word_count` are zero.
Otherwise `bits = ceil(log2(palette_count))` and
`word_count = ceil(4096 * bits / 64)`. Values may straddle adjacent words;
unused high bits in the final word are zero. Every decoded index must be less
than `palette_count`. Core assigns palette indices in first-cell-occurrence
order, then packs each index little-endian into the contiguous bit stream.

Minecraft keeps only the greatest revision for a given `(section x, y, z)` and
abandons a queued or in-progress older replacement when
a newer revision arrives. It loads or generates the containing chunk, installs
the decoded section as chunk data, marks it dirty, and sends the appropriate
`SCLA`. It targets only the Overworld.
