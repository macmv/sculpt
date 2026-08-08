package net.macmv.sculpt.live

import net.fabricmc.fabric.api.networking.v1.PlayerLookup
import net.minecraft.core.SectionPos
import io.netty.buffer.Unpooled
import net.minecraft.network.FriendlyByteBuf
import net.minecraft.network.protocol.game.ClientboundForgetLevelChunkPacket
import net.minecraft.network.protocol.game.ClientboundLevelChunkWithLightPacket
import net.minecraft.server.MinecraftServer
import net.minecraft.server.level.ServerLevel
import net.minecraft.world.level.Level
import net.minecraft.world.level.chunk.LevelChunkSection
import net.minecraft.world.level.levelgen.Heightmap
import org.slf4j.LoggerFactory

/** Installs core-produced sections directly into loaded/generated Overworld chunks. */
internal class LiveSectionReceiver {
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

  /** Called from END_SERVER_TICK. A tick installs only a small number of whole sections. */
  fun tick(server: MinecraftServer, maxSections: Int = 2) {
    val world = server.getLevel(Level.OVERWORLD) ?: return
    repeat(maxSections) {
      val delta = queue.poll() ?: return
      logger.info("Dequeued Sculpt section ({}, {}, {}) for revision {}", delta.key.x, delta.key.y, delta.key.z, delta.revision)
      if (queue.isSuperseded(delta)) {
        logger.info("Skipped superseded Sculpt section ({}, {}, {}) for revision {}", delta.key.x, delta.key.y, delta.key.z, delta.revision)
      } else if (install(world, delta)) {
        queue.markApplied(delta)
      } else {
        logger.warn("Failed to install Sculpt section ({}, {}, {}) for revision {}", delta.key.x, delta.key.y, delta.key.z, delta.revision)
      }
    }
  }

  private fun install(world: ServerLevel, delta: SectionDelta): Boolean = try {
    // `getSectionIndex` takes a block Y; the delta already carries section Y.
    val sectionIndex = world.getSectionIndexFromSectionY(delta.key.y)
    val chunk = world.getChunk(delta.key.x, delta.key.z)
    if (sectionIndex !in chunk.sections.indices) return false

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
    if (queue.isSuperseded(delta)) return false
    val sectionBottomY = delta.key.y shl 4
    // A replacement cannot retain block entities from the old section. Core's
    // format intentionally carries state only, so entity-bearing states are rejected.
    chunk.blockEntities.keys.filter { it.y in sectionBottomY until sectionBottomY + 16 }
      .toList().forEach(chunk::removeBlockEntity)
    chunk.sections[sectionIndex] = replacement
    Heightmap.primeHeightmaps(chunk, Heightmap.Types.values().toSet())
    chunk.markUnsaved()
    world.chunkSource.lightEngine.updateSectionStatus(SectionPos.of(delta.key.x, delta.key.y, delta.key.z), replacement.hasOnlyAir())
    world.chunkSource.lightEngine.propagateLightSources(chunk.pos)
    // A chunk-data packet alone is ignored by an already-loaded client in this
    // path. Make each tracker unload it first, then send the complete native
    // chunk and light state so its client-side chunk/render data is rebuilt.
    val packet = ClientboundLevelChunkWithLightPacket(chunk, world.chunkSource.lightEngine, null, null)
    val forget = ClientboundForgetLevelChunkPacket(chunk.pos)
    PlayerLookup.tracking(world, chunk.pos).forEach {
      it.connection.send(forget)
      it.connection.send(packet)
    }
    logger.info(
      "Installed Sculpt section ({}, {}, {}) for revision {}",
      delta.key.x,
      delta.key.y,
      delta.key.z,
      delta.revision,
    )
    true
  } catch (error: Exception) {
    logger.warn("Exception installing Sculpt section ({}, {}, {}) for revision {}", delta.key.x, delta.key.y, delta.key.z, delta.revision, error)
    false
  }
}
