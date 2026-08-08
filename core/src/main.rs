use std::{fs, io, os::unix::net::UnixListener, path::Path};

const SOCKET_PATH: &str = "/tmp/sculpt-live.sock";

fn main() -> Result<(), Box<dyn std::error::Error>> {
  setup_logging()?;

  remove_stale_socket(Path::new(SOCKET_PATH))?;
  let listener = UnixListener::bind(SOCKET_PATH)?;
  log::info!("listening for Blender connections at {SOCKET_PATH}");

  for connection in listener.incoming() {
    match connection {
      Ok(_) => log::info!("received Blender connection"),
      Err(error) => log::warn!("failed to accept Blender connection: {error}"),
    }
  }

  Ok(())
}

fn setup_logging() -> Result<(), fern::InitError> {
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
