# Sculpt Live

Sculpt Live is a local Blender-to-Minecraft terrain workflow. Sculpt a closed mesh in Blender using its normal tools (including Dyntopo), publish the evaluated mesh to the included Rust service, and the Fabric mod receives voxelized section updates to apply in Minecraft. Blender remains the source of truth; Minecraft is a live, block-based view of the sculpt.

## Installation

Sculpt Live currently runs from source on Linux or another Unix-like system with Unix domain sockets.

1. Install the prerequisites:
   - Blender
   - A current Rust toolchain (edition 2024 support)
   - JDK 26, for the Minecraft mod build
   - Minecraft `26.2` with the Fabric Loader, Fabric API, and Fabric Language Kotlin versions declared in [`minecraft/gradle.properties`](minecraft/gradle.properties)

2. Build and start the local voxel service. It listens on `/tmp/sculpt-live.sock`:

   ```sh
   cd core
   cargo run --release
   ```

3. Install the Blender add-on. In Blender, open **Edit > Preferences > Add-ons**, choose **Install from Disk**, select the `blender/sculpt_live` directory (or a zip containing that directory), then enable **Sculpt Live**. In the 3D View sidebar, select a mesh source and use **Publish Snapshot**.

4. Build the Minecraft mod and place the resulting JAR from `minecraft/build/libs/` in the target instance's `mods` directory:

   ```sh
   cd minecraft
   ./gradlew build
   ```

5. Start Minecraft and join or create a world. The mod connects to the same local socket service and applies incoming sculpt updates on the server side.

For the wire format and coordinate mapping, see [`protocol.md`](protocol.md). For the architectural design, see [`design.md`](design.md).
