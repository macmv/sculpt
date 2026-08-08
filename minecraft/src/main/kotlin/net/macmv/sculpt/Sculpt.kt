package net.macmv.sculpt

import net.fabricmc.api.ModInitializer
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerLifecycleEvents
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerTickEvents
import net.macmv.sculpt.live.LiveSectionReceiver

class Sculpt : ModInitializer {
  private val receiver = LiveSectionReceiver()

  override fun onInitialize() {
    ServerLifecycleEvents.SERVER_STARTED.register { receiver.start() }
    ServerLifecycleEvents.SERVER_STOPPING.register { receiver.stop() }
    ServerTickEvents.END_SERVER_TICK.register { receiver.tick(it) }
  }
}
