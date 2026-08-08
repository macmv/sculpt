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
See `../PROTOCOL.md` for the wire format.
The intended transport is a Unix domain socket at `/tmp/sculpt-live.sock`, with
length-prefixed binary messages. Do not send mesh geometry as JSON.

## Layout

- `sculpt_live/__init__.py` — add-on metadata and registration
- `sculpt_live/properties.py` — scene-level configuration
- `sculpt_live/operators.py` — evaluated-mesh snapshot entry point
- `sculpt_live/protocol.py` — binary full-mesh snapshot encoder
- `sculpt_live/transport.py` — Unix-domain-socket sender
- `sculpt_live/ui.py` — 3D View panel

Keep Blender as the source of truth. The eventual transport should send binary
full mesh snapshots to the local Rust service, not JSON geometry.
