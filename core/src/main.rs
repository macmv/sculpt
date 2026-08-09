mod blender;
mod minecraft;
mod topology;

use std::{
  fs,
  io::{self, Read, Write},
  os::unix::{
    fs::FileTypeExt,
    net::{UnixListener, UnixStream},
  },
  path::Path,
  sync::mpsc::{self, Receiver, SyncSender, TrySendError},
  thread,
};

const SOCKET_PATH: &str = "/tmp/sculpt-live.sock";
const MAX_FRAME_SIZE: usize = 4 * 1024 * 1024;
/// Bounds per-subscriber memory without truncating ordinary full-sculpt
/// updates, which commonly touch far more than 128 sections.
const SUBSCRIBER_QUEUE_CAPACITY: usize = 16_384;

enum CoreEvent {
  Snapshot(blender::MeshSnapshot),
  Subscribe { subscriber: minecraft::MinecraftSubscriber, outgoing: SyncSender<OutgoingDelta> },
}

struct Subscriber {
  subscriber: minecraft::MinecraftSubscriber,
  outgoing:   SyncSender<OutgoingDelta>,
}

struct OutgoingDelta {
  revision: u64,
  position: minecraft::SectionPos,
  payload:  Vec<u8>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
  setup_logging()?;

  remove_stale_socket(Path::new(SOCKET_PATH))?;
  let listener = UnixListener::bind(SOCKET_PATH)?;
  log::info!("listening for connections at {SOCKET_PATH}");

  let (sender, receiver) = mpsc::channel();
  thread::spawn(move || accept_connections(listener, sender));
  process_events(receiver);
  Ok(())
}

fn accept_connections(listener: UnixListener, sender: mpsc::Sender<CoreEvent>) {
  for connection in listener.incoming() {
    match connection {
      Ok(stream) => {
        let sender = sender.clone();
        thread::spawn(move || receive_connection(stream, sender));
      }
      Err(error) => log::warn!("failed to accept connection: {error}"),
    }
  }
}

fn receive_connection(mut stream: UnixStream, sender: mpsc::Sender<CoreEvent>) {
  log::info!("received socket client connection");
  let first = match read_frame(&mut stream) {
    Ok(frame) => frame,
    Err(error) => {
      log::warn!("failed to read initial packet: {error}");
      return;
    }
  };

  match first.get(..4) {
    Some(b"SCLP") => receive_snapshots(stream, first, sender),
    Some(b"SCLM") => receive_subscriber(stream, first, sender),
    _ => log::warn!("discarded connection with unknown initial packet"),
  }
}

fn receive_snapshots(mut stream: UnixStream, first: Vec<u8>, sender: mpsc::Sender<CoreEvent>) {
  let mut frame = Some(first);
  loop {
    let bytes = match frame.take() {
      Some(frame) => frame,
      None => match read_frame(&mut stream) {
        Ok(frame) => frame,
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
          log::info!("Blender connection closed");
          return;
        }
        Err(error) => {
          log::warn!("discarded invalid Blender message: {error}");
          return;
        }
      },
    };

    match blender::parse_mesh_snapshot(&bytes) {
      Ok(snapshot) => {
        if sender.send(CoreEvent::Snapshot(snapshot)).is_err() {
          return;
        }
      }
      Err(error) => {
        log::warn!("discarded invalid Blender message: {error}");
        return;
      }
    }
  }
}

fn receive_subscriber(mut stream: UnixStream, hello: Vec<u8>, sender: mpsc::Sender<CoreEvent>) {
  let subscriber = match minecraft::MinecraftSubscriber::new(&hello, SUBSCRIBER_QUEUE_CAPACITY) {
    Ok(subscriber) => subscriber,
    Err(error) => {
      log::warn!("discarded invalid Minecraft hello: {error}");
      return;
    }
  };
  if let Err(error) = write_frame(&mut stream, &minecraft::welcome()) {
    log::warn!("failed to welcome Minecraft subscriber: {error}");
    return;
  }

  let (outgoing, messages) = mpsc::sync_channel(SUBSCRIBER_QUEUE_CAPACITY);
  if sender.send(CoreEvent::Subscribe { subscriber, outgoing }).is_err() {
    return;
  }
  for delta in messages {
    if let Err(error) = write_frame(&mut stream, &delta.payload) {
      log::info!("Minecraft subscriber disconnected: {error}");
      return;
    }
    log::info!(
      "sent Minecraft section ({}, {}, {}) for revision {}",
      delta.position.x,
      delta.position.y,
      delta.position.z,
      delta.revision
    );
  }
}

fn process_events(receiver: Receiver<CoreEvent>) {
  let mut latest_revision = None;
  let mut subscribers = Vec::<Subscriber>::new();
  for event in receiver {
    match event {
      CoreEvent::Subscribe { subscriber, outgoing } => {
        log::info!("registered Minecraft subscriber");
        subscribers.push(Subscriber { subscriber, outgoing });
      }
      CoreEvent::Snapshot(snapshot) => {
        if latest_revision.is_some_and(|revision| snapshot.revision <= revision) {
          log::warn!("discarded stale Blender snapshot revision {}", snapshot.revision);
          continue;
        }

        subscribers.retain_mut(|entry| {
          if let Err(error) = topology::reconcile(&snapshot, None, entry.subscriber.state_mut()) {
            log::warn!(
              "failed to reconcile Minecraft subscriber for revision {}: {error}",
              snapshot.revision
            );
            return true;
          }
          let changed_sections = entry.subscriber.enqueue_modified_sections(snapshot.revision);
          log::info!(
            "finished topology reconciliation for revision {} ({} changed sections)",
            snapshot.revision,
            changed_sections
          );
          for (position, payload) in entry.subscriber.take_pending() {
            let delta = OutgoingDelta { revision: snapshot.revision, position, payload };
            match entry.outgoing.try_send(delta) {
              Ok(()) => {}
              Err(TrySendError::Full(_)) => {
                log::warn!(
                  "Minecraft subscriber queue reached its {SUBSCRIBER_QUEUE_CAPACITY}-section limit; dropping section ({}, {}, {}) for revision {}",
                  position.x,
                  position.y,
                  position.z,
                  snapshot.revision
                );
              }
              Err(TrySendError::Disconnected(_)) => return false,
            }
          }
          true
        });

        latest_revision = Some(snapshot.revision);
        log::info!(
          "received Blender mesh snapshot revision {} ({} vertices, {} triangles)",
          snapshot.revision,
          snapshot.vertices.len(),
          snapshot.triangles.len()
        );
      }
    }
  }
}

fn read_frame(reader: &mut impl Read) -> io::Result<Vec<u8>> {
  let mut length = [0; 4];
  reader.read_exact(&mut length)?;
  let length = u32::from_le_bytes(length) as usize;
  if length == 0 || length > MAX_FRAME_SIZE {
    return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid frame length"));
  }
  let mut frame = vec![0; length];
  reader.read_exact(&mut frame)?;
  Ok(frame)
}

fn write_frame(writer: &mut impl Write, payload: &[u8]) -> io::Result<()> {
  let length = u32::try_from(payload.len())
    .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame is too large"))?;
  writer.write_all(&length.to_le_bytes())?;
  writer.write_all(payload)
}

fn setup_logging() -> Result<(), log::SetLoggerError> {
  fern::Dispatch::new()
    .format(|out, message, record| {
      out.finish(format_args!("[{}] {}: {}", record.level(), record.target(), message))
    })
    .level(log::LevelFilter::Info)
    .chain(io::stdout())
    .apply()
}

fn remove_stale_socket(path: &Path) -> io::Result<()> {
  match fs::symlink_metadata(path) {
    Ok(metadata) if metadata.file_type().is_socket() => fs::remove_file(path),
    Ok(_) => Err(io::Error::new(
      io::ErrorKind::AlreadyExists,
      format!("refusing to replace non-socket path {}", path.display()),
    )),
    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
    Err(error) => Err(error),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn frames_are_little_endian_and_strict() {
    let mut bytes = Vec::new();
    write_frame(&mut bytes, b"SCLM").unwrap();
    assert_eq!(bytes, [4, 0, 0, 0, b'S', b'C', b'L', b'M']);
    assert_eq!(read_frame(&mut bytes.as_slice()).unwrap(), b"SCLM");
    assert!(read_frame(&mut [0, 0, 0, 0].as_slice()).is_err());
  }
}
