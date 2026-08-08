package net.macmv.sculpt.live

import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.charset.StandardCharsets
import net.minecraft.core.registries.BuiltInRegistries
import net.minecraft.world.level.block.state.properties.Property

internal const val SECTION_VOLUME = 4096
internal data class SectionKey(val x: Int, val y: Int, val z: Int)
internal data class SectionDelta(val key: SectionKey, val revision: Long, val palette: List<String>, val indices: IntArray)

/** Strict decoder for the unversioned `SCLD` section replacement payload. */
internal object Protocol {
  const val MAX_FRAME_BYTES = 4 * 1024 * 1024

  /** Complete local block-state registry, carried in the one `SCLM` hello frame. */
  fun hello(): ByteArray {
    val states = BuiltInRegistries.BLOCK.asHolderIdMap().map { it.value() }
      .flatMap { it.stateDefinition.possibleStates }
      .map(::encodeBlockState).distinct().sorted()
    require(states.size in 1..0xffff) { "Invalid block-state registry size" }
    val encoded = states.map { it.toByteArray(StandardCharsets.UTF_8) }
    require(encoded.all { it.isNotEmpty() && it.size <= 0xffff }) { "Invalid block-state registry entry" }
    return ByteBuffer.allocate(6 + encoded.sumOf { 2 + it.size }).order(ByteOrder.LITTLE_ENDIAN)
      .put("SCLM".toByteArray(StandardCharsets.US_ASCII)).putShort(states.size.toShort()).also { output ->
        encoded.forEach { output.putShort(it.size.toShort()).put(it) }
      }.array()
  }

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

  fun isWelcome(payload: ByteArray): Boolean = payload.size == 8 &&
    ByteBuffer.wrap(payload).order(ByteOrder.LITTLE_ENDIAN).let { readMagic(it) == "SCLW" && it.int == 0 }

  fun decodeDelta(payload: ByteArray): SectionDelta {
    val input = ByteBuffer.wrap(payload).order(ByteOrder.LITTLE_ENDIAN)
    require(input.remaining() >= 26) { "Truncated SCLD" }
    require(readMagic(input) == "SCLD") { "Expected SCLD" }
    val revision = input.long
    val key = SectionKey(input.int, input.int, input.int)
    val paletteSize = input.short.toInt() and 0xffff
    require(paletteSize in 1..SECTION_VOLUME) { "Invalid palette size" }
    val palette = ArrayList<String>(paletteSize)
    repeat(paletteSize) {
      require(input.remaining() >= 2) { "Truncated palette length" }
      val length = input.short.toInt() and 0xffff
      require(length in 1..32767 && input.remaining() >= length) { "Invalid palette entry" }
      val bytes = ByteArray(length); input.get(bytes)
      palette += bytes.toString(StandardCharsets.UTF_8)
    }
    require(input.remaining() >= 3) { "Truncated packed indices" }
    val bits = input.get().toInt() and 0xff
    val words = input.short.toInt() and 0xffff
    val expectedBits = if (paletteSize == 1) 0 else 32 - Integer.numberOfLeadingZeros(paletteSize - 1)
    val expectedWords = if (bits == 0) 0 else (SECTION_VOLUME * bits + 63) / 64
    require(bits == expectedBits && words == expectedWords && input.remaining() == words * 8) { "Invalid packed indices" }
    val packed = LongArray(words) { input.long }
    val indices = IntArray(SECTION_VOLUME)
    for (cell in indices.indices) {
      if (bits == 0) break
      val offset = cell * bits
      val word = offset ushr 6
      val shift = offset and 63
      var value = (packed[word] ushr shift).toInt()
      if (shift + bits > 64) value = value or (packed[word + 1] shl (64 - shift)).toInt()
      value = value and ((1 shl bits) - 1)
      require(value < paletteSize) { "Palette index out of range" }
      indices[cell] = value
    }
    return SectionDelta(key, revision, palette, indices)
  }

  private fun readMagic(input: ByteBuffer) = ByteArray(4).also(input::get).toString(StandardCharsets.US_ASCII)
}
