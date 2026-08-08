# Blender-to-Minecraft Live Sculpting

## Goal

Use Blender as the sculpting application and see the resulting terrain appear in
Minecraft with low latency. Blender's sculpt tools, especially Dyntopo, are the
reason for this project; this is not a replacement sculpting UI and does not use
Blender's Blocks Remesh modifier.

Minecraft is a derived, block-based representation of the Blender sculpt. The
Blender scene is the source of truth.

## System shape

```text
Blender + Python add-on  ->  local Rust voxel service  ->  Minecraft server mod
       source mesh              voxel cache/diff              world updates
```

- **Blender** provides the complete existing sculpting workflow: brushes,
  Dyntopo, tablet input, masking, symmetry, and modifiers.
- **The Blender add-on** extracts the current evaluated mesh and publishes it to
  the local service. It owns Blender UI, configuration, and change detection;
  it does not perform CPU-heavy voxelization.
- **The Rust service** owns the current mesh snapshot, acceleration structure,
  voxel/chunk cache, block-material rules, and diffing. It produces compact
  Minecraft chunk deltas.
- **The Minecraft mod** receives validated deltas and applies them safely in
  bounded batches. It should not perform mesh conversion or expensive diffing.

Keeping Rust in a separate local process is preferable to loading it in Blender
or the JVM initially: one implementation serves both sides, native failures do
not crash Blender/the server, and there is no JNI or Blender-Python ABI
packaging burden.

## Blender to Rust protocol

The transport is local IPC (Unix socket/named pipe or localhost WebSocket) with
a binary protocol. Do not send geometry as JSON.

Every message includes a sculpt-object identifier, monotonically increasing
revision, object transform, and a world-space dirty AABB hint.

### Dyntopo: normal path

Dyntopo can create, remove, and re-triangulate faces on every stroke. There is
no useful or reliable general-purpose topology diff between the old and new
mesh. On stroke end, Blender sends a full current mesh snapshot:

```text
MeshSnapshot {
  revision,
  vertices: float32 xyz[],
  triangles: uint32 index triplets[],
  face_material_ids[],
  transform,
  dirty_aabb,
}
```

Rust replaces the mesh snapshot for that revision and rebuilds its triangle
BVH. It uses the dirty AABB to limit *voxel recomputation*, not to limit what
geometry is required for correct inside/outside classification.

Start with publishing after a stroke ends. Later, add debounced live publishing
(for example every 250--500 ms) and cancel obsolete work when a newer revision
arrives.

### Possible later optimization

For non-Dyntopo editing where the vertex/index layout is known to be stable,
the add-on may send a changed position buffer instead of a full snapshot. Rust
keeps the index buffer and refits/rebuilds its BVH. This is explicitly an
optimization, not a requirement; topology changes always fall back to a full
snapshot.

## Voxelization and diffing

Do not diff Blender topology. Diff the output block state.

```text
current mesh + padded dirty AABB
  -> find intersecting Minecraft chunks
  -> voxelize those chunks from the current mesh
  -> compare with cached previous chunk block IDs
  -> emit only changed blocks/runs/chunks
```

The Rust service stores sparse, palette-compressed block chunks (initially use
Minecraft's 16x16x16 section granularity). Hash a newly generated chunk before
performing a cell-by-cell comparison; identical chunks produce no network
update.

The dirty region comes from the sculpt stroke path plus brush radius, converted
to world space and padded by one or more Minecraft blocks. It is deliberately
conservative. Whole-object invalidation is used for changes with non-local
effects, such as global modifier changes, transform changes (invalidate old and
new bounds), or an unknown edit.

Voxelization needs a well-defined solid. The initial workflow should use a
watertight sculpt mesh and a documented inside/outside test. Non-manifold or
open meshes need an explicitly chosen fallback rule rather than implicit,
unpredictable behavior.

## Materials

Shape and block selection are separate concerns.

- The mesh determines solid versus air.
- Face materials and/or vertex-color masks can select block families.
- Rules can resolve terrain layering: grass on exposed upward-facing surfaces,
  dirt for a shallow depth, stone below, and snow based on elevation/slope.
- Explicit painting should override procedural material rules.

## Minecraft application

The Minecraft side is a server-side Fabric/NeoForge mod (including the
integrated server for local single-player use). It accepts only local,
versioned chunk deltas, queues them, and applies them in bounded batches on the
safe server execution path. It must not edit region files directly while the
world is running.

Large changes are progressive: update nearby/visible chunks first where useful,
then stream the remainder without monopolizing the game thread.

## Milestones

1. One closed Blender sculpt object, fixed origin/scale, one solid Minecraft
   block type, manual **Publish**.
2. Rust receives full mesh snapshots, voxelizes the whole object, and exports
   or sends chunk results.
3. Minecraft mod receives and applies complete chunk updates safely.
4. Add stroke-end dirty-AABB voxelization and output-chunk diffs.
5. Add material/terrain-layer rules and debounced live updates.
6. Profile before optimizing: consider stable-topology mesh deltas, BVH refits,
   lower-detail proxy source meshes, or GPU voxelization only if measurements
   justify them.

## Non-goals for the first version

- A custom sculpting UI.
- Blender Blocks Remesh or one Blender cube object per Minecraft block.
- General arbitrary-world import/edit synchronization back from Minecraft.
- Incremental topology patching for Dyntopo.
