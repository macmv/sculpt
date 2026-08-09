package net.macmv.sculpt.live

import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.charset.StandardCharsets
import net.minecraft.core.registries.BuiltInRegistries
import net.minecraft.world.level.block.Block
import net.minecraft.world.level.block.state.properties.Property

internal const val SECTION_VOLUME = 4096
internal data class SectionKey(val x: Int, val y: Int, val z: Int)
internal data class SectionDelta(val key: SectionKey, val revision: Long, val sectionData: ByteArray)

/** Strict decoder for the unversioned `SCLD` section replacement payload. */
internal object Protocol {
  const val MAX_FRAME_BYTES = 4 * 1024 * 1024

  /** Complete local block-state registry, carried in the one `SCLM` hello frame. */
  fun hello(): ByteArray {
    // IDs in this order are the global palette IDs used by PalettedContainer.
    val states = (0 until Block.BLOCK_STATE_REGISTRY.size()).map { id ->
      encodeBlockState(requireNotNull(Block.BLOCK_STATE_REGISTRY.byId(id)) { "Missing global block-state ID $id" })
    }
    require(states.size in 1..(1 shl 15)) { "Invalid block-state registry size" }
    val encoded = states.map { it.toByteArray(StandardCharsets.UTF_8) }
    val defaults = BuiltInRegistries.BLOCK.mapNotNull { block ->
      val state = block.defaultBlockState()
      if (state.properties.isEmpty()) null else {
        val id = Block.BLOCK_STATE_REGISTRY.getId(state)
        require(id >= 0) { "Missing global block-state ID for default state of $block" }
        BuiltInRegistries.BLOCK.getKey(block).toString().toByteArray(StandardCharsets.UTF_8) to id
      }
    }
    require(encoded.all { it.isNotEmpty() && it.size <= 0xffff }) { "Invalid block-state registry entry" }
    require(defaults.size <= 0xffff && defaults.all { (name, _) -> name.isNotEmpty() && name.size <= 0xffff }) {
      "Invalid default block-state registry entry"
    }
    return ByteBuffer.allocate(8 + encoded.sumOf { 2 + it.size } + defaults.sumOf { (name, _) -> 4 + name.size })
      .order(ByteOrder.LITTLE_ENDIAN)
      .put("SCLM".toByteArray(StandardCharsets.US_ASCII)).putShort(states.size.toShort()).also { output ->
        encoded.forEach { output.putShort(it.size.toShort()).put(it) }
        output.putShort(defaults.size.toShort())
        defaults.forEach { (name, id) -> output.putShort(id.toShort()).putShort(name.size.toShort()).put(name) }
      }.array()
  }

  fun isWelcome(payload: ByteArray): Boolean = payload.size == 8 &&
    ByteBuffer.wrap(payload).order(ByteOrder.LITTLE_ENDIAN).let { readMagic(it) == "SCLW" && it.int == 0 }

  fun decodeDelta(payload: ByteArray): SectionDelta {
    val input = ByteBuffer.wrap(payload).order(ByteOrder.LITTLE_ENDIAN)
    require(input.remaining() >= 24) { "Truncated SCLD" }
    require(readMagic(input) == "SCLD") { "Expected SCLD" }
    val revision = input.long
    val key = SectionKey(input.int, input.int, input.int)
    val sectionData = ByteArray(input.remaining())
    require(sectionData.isNotEmpty()) { "Truncated section data" }
    input.get(sectionData)
    return SectionDelta(key, revision, sectionData)
  }

  private fun readMagic(input: ByteBuffer) = ByteArray(4).also(input::get).toString(StandardCharsets.US_ASCII)

  /** Human-readable lookup names only; their order above is the actual wire ID mapping. */
  private fun encodeBlockState(state: net.minecraft.world.level.block.state.BlockState): String {
    val id = BuiltInRegistries.BLOCK.getKey(state.block).toString()
    if (state.properties.isEmpty()) return id
    return id + state.properties.sortedBy { it.name }.joinToString(",", prefix = "[", postfix = "]") { property ->
      "${property.name}=${propertyValue(state, property)}"
    }
  }

  @Suppress("UNCHECKED_CAST")
  private fun propertyValue(state: net.minecraft.world.level.block.state.BlockState, property: Property<*>): String {
    val typed = property as Property<Comparable<Any>>
    return typed.getName(state.getValue(typed))
  }
}
