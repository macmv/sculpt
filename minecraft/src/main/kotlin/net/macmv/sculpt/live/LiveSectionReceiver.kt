package net.macmv.sculpt.live

import net.fabricmc.fabric.api.networking.v1.PlayerLookup
import net.minecraft.core.registries.BuiltInRegistries
import net.minecraft.core.SectionPos
import net.minecraft.network.protocol.game.ClientboundLevelChunkWithLightPacket
import net.minecraft.resources.Identifier
import net.minecraft.server.MinecraftServer
import net.minecraft.server.level.ServerLevel
import net.minecraft.world.level.Level
import net.minecraft.world.level.block.state.BlockState
import net.minecraft.world.level.block.state.properties.Property
import net.minecraft.world.level.chunk.LevelChunkSection
import net.minecraft.world.level.levelgen.Heightmap

/** Installs core-produced sections directly into loaded/generated Overworld chunks. */
internal class LiveSectionReceiver {
  private val queue = DeltaQueue()
  private val socket = SocketSubscriber(queue)

  fun start() = socket.start()
  fun stop() = socket.stop()

  /** Called from END_SERVER_TICK. A tick installs only a small number of whole sections. */
  fun tick(server: MinecraftServer, maxSections: Int = 2) {
    val world = server.getLevel(Level.OVERWORLD) ?: return
    repeat(maxSections) {
      val delta = queue.poll() ?: return
      if (!queue.isSuperseded(delta) && install(world, delta)) queue.markApplied(delta)
    }
  }

  private fun install(world: ServerLevel, delta: SectionDelta): Boolean = try {
    val states = delta.palette.map(::parseBlockState)
    require(states.none { it.hasBlockEntity() }) { "Block entities are not supported in section deltas" }
    val sectionIndex = world.getSectionIndex(delta.key.y)
    val chunk = world.getChunk(delta.key.x, delta.key.z)
    if (sectionIndex !in chunk.sections.indices) return false

    // This deliberately bypasses ServerWorld.setBlockState: build a replacement
    // section off-world, then swap it into the chunk in one operation.
    val replacement: LevelChunkSection = chunk.getSection(sectionIndex).copy()
    for (cell in 0 until SECTION_VOLUME) {
      val x = cell and 15
      val z = (cell ushr 4) and 15
      val y = cell ushr 8
      replacement.setBlockState(x, y, z, states[delta.indices[cell]], false)
    }
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
    // Send one complete chunk packet to clients which already track this chunk.
    // No block mutation or per-cell update packets are emitted.
    val packet = ClientboundLevelChunkWithLightPacket(chunk, world.chunkSource.lightEngine, null, null)
    PlayerLookup.tracking(world, chunk.pos).forEach { it.connection.send(packet) }
    true
  } catch (_: IllegalArgumentException) {
    false
  }

  private fun parseBlockState(encoded: String): BlockState {
    val bracket = encoded.indexOf('[')
    val idText = if (bracket == -1) encoded else encoded.substring(0, bracket)
    val id = Identifier.parse(idText)
    val block = BuiltInRegistries.BLOCK.getOptional(id).orElseThrow { IllegalArgumentException("Unknown block: $idText") }
    var state = block.defaultBlockState()
    if (bracket == -1) return state
    require(encoded.endsWith(']')) { "Malformed block state: $encoded" }
    val properties = encoded.substring(bracket + 1, encoded.length - 1)
    if (properties.isEmpty()) return state
    for (part in properties.split(',')) {
      val equals = part.indexOf('=')
      require(equals > 0 && equals < part.length - 1) { "Malformed block property: $part" }
      state = setProperty(state, part.substring(0, equals), part.substring(equals + 1))
    }
    return state
  }

  private fun setProperty(state: BlockState, name: String, value: String): BlockState {
    val property = state.properties.firstOrNull { it.name == name } ?: throw IllegalArgumentException("Unknown block property: $name")
    return setParsedProperty(state, property, value)
  }

  @Suppress("UNCHECKED_CAST")
  private fun setParsedProperty(state: BlockState, property: Property<*>, value: String): BlockState {
    val typed = property as Property<Comparable<Any>>
    val parsed = typed.getValue(value).orElseThrow { IllegalArgumentException("Invalid value '$value' for ${property.name}") }
    return state.setValue(typed, parsed)
  }
}
