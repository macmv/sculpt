package net.macmv.sculpt.live

import net.fabricmc.fabric.api.networking.v1.PlayerLookup
import net.minecraft.core.BlockPos
import net.minecraft.core.SectionPos
import io.netty.buffer.Unpooled
import net.minecraft.network.FriendlyByteBuf
import net.minecraft.network.protocol.game.ClientboundLevelChunkWithLightPacket
import net.minecraft.server.MinecraftServer
import net.minecraft.server.level.ServerLevel
import net.minecraft.world.level.Level
import net.minecraft.world.level.chunk.LevelChunk
import net.minecraft.world.level.chunk.LevelChunkSection
import net.minecraft.world.level.levelgen.Heightmap
import org.slf4j.LoggerFactory

/** Installs core-produced sections directly into loaded/generated Overworld chunks. */
internal class LiveSectionReceiver {
  private companion object {
    const val SECTIONS_PER_TICK = 16
  }

  private val queue = DeltaQueue()
  private val socket = SocketSubscriber(queue)
  private val logger = LoggerFactory.getLogger(LiveSectionReceiver::class.java)

  fun start() {
    logger.info("Starting Sculpt section receiver")
    socket.start()
  }
  fun stop() {
    logger.info("Stopping Sculpt section receiver")
    socket.stop()
  }

  /** Called from END_SERVER_TICK. Work is bounded to keep normal simulation responsive. */
  fun tick(server: MinecraftServer, maxSections: Int = SECTIONS_PER_TICK) {
    val world = server.getLevel(Level.OVERWORLD) ?: return
    val changedChunks = LinkedHashSet<LevelChunk>()
    for (ignored in 0 until maxSections) {
      val delta = queue.poll() ?: break
      if (queue.isSuperseded(delta)) {
        logger.debug("Skipped superseded Sculpt section ({}, {}, {}) for revision {}", delta.key.x, delta.key.y, delta.key.z, delta.revision)
      } else if (install(world, delta)?.also(changedChunks::add) != null) {
        queue.markApplied(delta)
      } else {
        logger.warn("Failed to install Sculpt section ({}, {}, {}) for revision {}", delta.key.x, delta.key.y, delta.key.z, delta.revision)
      }
    }
    changedChunks.forEach { finishChunkUpdate(world, it) }
  }

  private fun install(world: ServerLevel, delta: SectionDelta): LevelChunk? = try {
    // `getSectionIndex` takes a block Y; the delta already carries section Y.
    val sectionIndex = world.getSectionIndexFromSectionY(delta.key.y)
    val chunk = world.getChunk(delta.key.x, delta.key.z)
    if (sectionIndex !in chunk.sections.indices) return null

    // This deliberately bypasses ServerWorld.setBlockState: build a replacement
    // section off-world, then swap it into the chunk in one operation.
    val replacement: LevelChunkSection = chunk.getSection(sectionIndex).copy()
    // LevelChunkSection.read also consumes its biome container. Sculpt owns
    // only block states, so retain the target section's native biome bytes and
    // append them before handing the complete payload to Minecraft.
    val input = FriendlyByteBuf(Unpooled.buffer(delta.sectionData.size + 64))
    input.writeBytes(delta.sectionData)
    replacement.biomes.write(input)
    input.readerIndex(0)
    replacement.read(input)
    require(input.readableBytes() == 0) { "Trailing native section data" }
    if (queue.isSuperseded(delta)) return null
    val sectionBottomY = delta.key.y shl 4
    // A replacement cannot retain block entities from the old section. Core's
    // format intentionally carries state only, so entity-bearing states are rejected.
    chunk.blockEntities.keys.filter { it.y in sectionBottomY until sectionBottomY + 16 }
      .toList().forEach(chunk::removeBlockEntity)
    chunk.sections[sectionIndex] = replacement
    val lightEngine = world.chunkSource.lightEngine
    lightEngine.updateSectionStatus(SectionPos.of(delta.key.x, delta.key.y, delta.key.z), replacement.hasOnlyAir())
    // A section-status update only handles an empty/non-empty transition. The
    // old section was swapped wholesale, so every cell must be rechecked to
    // remove stale light and propagate new occlusion/emission correctly.
    val baseX = delta.key.x shl 4
    val baseY = delta.key.y shl 4
    val baseZ = delta.key.z shl 4
    for (y in 0 until 16) for (z in 0 until 16) for (x in 0 until 16) {
      lightEngine.checkBlock(BlockPos(baseX + x, baseY + y, baseZ + z))
    }
    logger.debug(
      "Installed Sculpt section ({}, {}, {}) for revision {}",
      delta.key.x,
      delta.key.y,
      delta.key.z,
      delta.revision,
    )
    chunk
  } catch (error: Exception) {
    logger.warn("Exception installing Sculpt section ({}, {}, {}) for revision {}", delta.key.x, delta.key.y, delta.key.z, delta.revision, error)
    null
  }

  /** Completes all mutations for a chunk once, even when several sections changed this tick. */
  private fun finishChunkUpdate(world: ServerLevel, chunk: LevelChunk) {
    Heightmap.primeHeightmaps(chunk, Heightmap.Types.values().toSet())
    chunk.markUnsaved()
    val lightEngine = world.chunkSource.lightEngine
    lightEngine.propagateLightSources(chunk.pos)
    // This packet replaces the client chunk data and queues its corresponding
    // light update; unlike a forget packet, it does not leave a transient hole
    // in the client's chunk cache/light engine.
    val packet = ClientboundLevelChunkWithLightPacket(chunk, lightEngine, null, null)
    PlayerLookup.tracking(world, chunk.pos).forEach { it.connection.send(packet) }
  }
}
