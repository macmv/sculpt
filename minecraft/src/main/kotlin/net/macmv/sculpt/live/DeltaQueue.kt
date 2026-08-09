package net.macmv.sculpt.live

import java.util.concurrent.ConcurrentHashMap

/** Latest-only inbox shared by the socket worker and the server thread. */
internal class DeltaQueue {
  private val pending = ConcurrentHashMap<SectionKey, SectionDelta>()
  private val applied = ConcurrentHashMap<SectionKey, Long>()

  fun offer(delta: SectionDelta) {
    if (applied[delta.key]?.let { it >= delta.revision } == true) return
    pending.compute(delta.key) { _, old -> if (old == null || old.revision < delta.revision) delta else old }
  }

  fun poll(predicate: (SectionKey) -> Boolean = { true }): SectionDelta? {
    val entry = pending.entries.firstOrNull { predicate(it.key) } ?: return null
    return entry.takeIf { pending.remove(it.key, it.value) }?.value
  }

  fun isSuperseded(delta: SectionDelta) = pending[delta.key]?.revision?.let { it > delta.revision } == true
  fun markApplied(delta: SectionDelta) { applied.merge(delta.key, delta.revision, ::maxOf) }
}
