package net.macmv.sculpt.live

import java.io.EOFException
import java.net.StandardProtocolFamily
import java.net.UnixDomainSocketAddress
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.channels.Channels
import java.nio.channels.SocketChannel
import java.nio.file.Path
import java.util.concurrent.atomic.AtomicBoolean
import org.slf4j.LoggerFactory

internal class SocketSubscriber(private val queue: DeltaQueue, private val socketPath: Path = Path.of("/tmp/sculpt-live.sock")) {
  private val running = AtomicBoolean(false)
  private val logger = LoggerFactory.getLogger(SocketSubscriber::class.java)
  @Volatile private var channel: SocketChannel? = null
  private var worker: Thread? = null

  fun start() {
    if (running.compareAndSet(false, true)) worker = Thread(::run, "Sculpt live socket").apply { isDaemon = true; start() }
  }
  fun stop() { running.set(false); channel?.close(); worker?.interrupt() }

  private fun run() {
    var backoff = 100L
    while (running.get()) try {
      logger.info("Connecting Sculpt socket subscriber to {}", socketPath)
      SocketChannel.open(StandardProtocolFamily.UNIX).use { socket ->
        channel = socket; socket.connect(UnixDomainSocketAddress.of(socketPath))
        logger.info("Connected Sculpt socket subscriber to {}", socketPath)
        writeFully(socket, frame(Protocol.hello()))
        logger.info("Sent Sculpt SCLM hello")
        backoff = 100L
        val input = Channels.newInputStream(socket)
        require(Protocol.isWelcome(readFrame(input))) { "Expected SCLW after SCLM" }
        logger.info("Received Sculpt SCLW welcome")
        while (running.get()) {
          val delta = Protocol.decodeDelta(readFrame(input))
          logger.info("Received Sculpt section ({}, {}, {}) for revision {}", delta.key.x, delta.key.y, delta.key.z, delta.revision)
          queue.offer(delta)
        }
      }
    } catch (error: Exception) {
      if (running.get()) logger.warn("Sculpt socket subscriber failed; retrying in {} ms", backoff, error)
      if (running.get()) try { Thread.sleep(backoff) } catch (_: InterruptedException) { }
      backoff = (backoff * 2).coerceAtMost(5_000L)
    } finally { channel = null }
  }

  private fun frame(payload: ByteArray) = ByteBuffer.allocate(payload.size + 4).order(ByteOrder.LITTLE_ENDIAN).putInt(payload.size).put(payload).array()
  private fun writeFully(socket: SocketChannel, bytes: ByteArray) {
    val buffer = ByteBuffer.wrap(bytes)
    while (buffer.hasRemaining()) socket.write(buffer)
  }
  private fun readFrame(input: java.io.InputStream): ByteArray {
    val header = input.readNBytes(4); if (header.size != 4) throw EOFException()
    val length = ByteBuffer.wrap(header).order(ByteOrder.LITTLE_ENDIAN).int
    require(length in 1..Protocol.MAX_FRAME_BYTES) { "Invalid frame length" }
    return input.readNBytes(length).also { if (it.size != length) throw EOFException() }
  }
}
