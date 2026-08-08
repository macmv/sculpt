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
evaluates the mesh and reserves a monotonically increasing revision; transport
to the Rust voxel service is deliberately left as the next implementation step.

## Layout

- `sculpt_live/__init__.py` — add-on metadata and registration
- `sculpt_live/properties.py` — scene-level configuration
- `sculpt_live/operators.py` — evaluated-mesh snapshot entry point
- `sculpt_live/ui.py` — 3D View panel

Keep Blender as the source of truth. The eventual transport should send binary
full mesh snapshots to the local Rust service, not JSON geometry.
