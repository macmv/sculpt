mod blender;
mod minecraft;
mod topology;

use std::{
  fs, io,
  os::unix::{
    fs::FileTypeExt,
    net::{UnixListener, UnixStream},
  },
  path::Path,
};

const SOCKET_PATH: &str = "/tmp/sculpt-live.sock";

fn main() -> Result<(), Box<dyn std::error::Error>> {
  setup_logging()?;

  remove_stale_socket(Path::new(SOCKET_PATH))?;
  let listener = UnixListener::bind(SOCKET_PATH)?;
  log::info!("listening for Blender connections at {SOCKET_PATH}");

  let mut latest_revision = None;
  for connection in listener.incoming() {
    match connection {
      Ok(stream) => receive_snapshots(stream, &mut latest_revision),
      Err(error) => log::warn!("failed to accept Blender connection: {error}"),
    }
  }

  Ok(())
}

fn receive_snapshots(stream: UnixStream, latest_revision: &mut Option<u64>) {
  log::info!("received Blender connection");
  let mut stream = stream;

  loop {
    match blender::read_mesh_snapshot(&mut stream) {
      Ok(snapshot) => {
        if latest_revision.is_some_and(|revision| snapshot.revision <= revision) {
          log::warn!("discarded stale Blender snapshot revision {}", snapshot.revision);
          continue;
        }

        *latest_revision = Some(snapshot.revision);
        log::info!(
          "parsed Blender mesh snapshot revision {} ({} vertices, {} triangles)",
          snapshot.revision,
          snapshot.vertices.len(),
          snapshot.triangles.len()
        );
      }
      Err(blender::ReadError::Io(error)) if error.kind() == io::ErrorKind::UnexpectedEof => {
        log::info!("Blender connection closed");
        return;
      }
      Err(error) => {
        log::warn!("discarded invalid Blender message: {error}");
        return;
      }
    }
  }
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
