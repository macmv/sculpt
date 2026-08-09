# Sculpt Live Blender Add-on

This directory contains the Blender source for the Blender-to-Minecraft Live
Sculpting add-on.

## Install for development

1. In Blender, open **Edit > Preferences > Add-ons**.
2. Select **Install from Disk** and choose the `blender/sculpt_live` directory
   (or package that directory as a zip first).
3. Enable **Sculpt Live**.

The initial scaffold exposes a **Sculpt Live** panel in the 3D View sidebar.
Choose a mesh source object and use **Publish Snapshot**. The current operator
builds a compact binary full-mesh snapshot and sends it to the configured Unix
domain socket. A revision advances only after the complete frame is written.
See `../protocol.md` for the wire format.

## Painted Materials

In the add-on panel, use **Painted Materials** to associate a paint color with
a base block, underground block, and positive base-layer depth. For example, a
grass-like material can use `minecraft:grass_block` over `minecraft:dirt` with
a depth of one or more blocks.
The Sculpt workspace's **Paint** brush paints the mesh's active color attribute
(vertex/corner color data); it does not assign Blender material slots and is not
texture paint. On publish, each polygon is matched to the nearest configured
color and the voxel service uses the selected material for that solid column:
the topmost `base_depth` solid blocks use its base block and deeper blocks use
its underground block. Add at least one material before publishing.
The intended transport is a Unix domain socket at `/tmp/sculpt-live.sock`, with
length-prefixed binary messages. Do not send mesh geometry as JSON.

## Layout

- `sculpt_live/__init__.py` — add-on metadata and registration
- `sculpt_live/properties.py` — scene-level configuration
- `sculpt_live/operators.py` — evaluated-mesh snapshot entry point
- `sculpt_live/protocol.py` — binary `SCLP` full-mesh snapshot encoder
- `sculpt_live/transport.py` — Unix-domain-socket sender
- `sculpt_live/ui.py` — 3D View panel

Keep Blender as the source of truth. The transport sends length-prefixed binary
full-mesh snapshots to the local Rust service, never JSON geometry. Protocol
frames are limited to 4 MiB.
