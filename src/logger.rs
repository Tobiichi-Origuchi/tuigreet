use std::{
  fs::{File, OpenOptions, Permissions},
  io,
  os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
  path::Path,
};

use tracing_appender::non_blocking::WorkerGuard;

pub fn init(debug: bool, path: impl AsRef<Path>) -> io::Result<Option<WorkerGuard>> {
  use tracing_subscriber::{
    filter::{LevelFilter, Targets},
    prelude::*,
  };

  let Some(file) = open_log_file(debug, path.as_ref())? else {
    return Ok(None);
  };

  let (appender, guard) = tracing_appender::non_blocking(file);
  let target = Targets::new().with_target("tuigreet", LevelFilter::DEBUG);

  tracing_subscriber::registry()
    .with(
      tracing_subscriber::fmt::layer()
        .with_writer(appender)
        .with_line_number(true),
    )
    .with(target)
    .try_init()
    .map_err(|error| io::Error::other(format!("failed to install tracing subscriber: {error}")))?;

  Ok(Some(guard))
}

fn open_log_file(debug: bool, path: &Path) -> io::Result<Option<File>> {
  if !debug {
    return Ok(None);
  }

  let file = OpenOptions::new()
    .create(true)
    .append(true)
    .mode(0o600)
    // O_NOFOLLOW closes the final-component symlink race. O_NONBLOCK makes
    // opening a FIFO or device fail promptly so that a malicious path cannot
    // stall the greeter before we validate its type.
    .custom_flags(nix::libc::O_CLOEXEC | nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
    .open(path)?;

  let metadata = file.metadata()?;
  if !metadata.file_type().is_file() {
    return Err(io::Error::new(
      io::ErrorKind::InvalidInput,
      "debug log is not a regular file",
    ));
  }
  if metadata.nlink() != 1 {
    return Err(io::Error::new(
      io::ErrorKind::InvalidInput,
      "debug log must not have multiple hard links",
    ));
  }

  // mode() only applies to newly created files. Tighten an existing log as
  // well, and reject it if the greeter is not allowed to make it private.
  file.set_permissions(Permissions::from_mode(0o600))?;

  Ok(Some(file))
}

#[cfg(test)]
mod tests {
  use std::{
    ffi::CString,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::{ffi::OsStrExt, fs::PermissionsExt},
  };

  use tempfile::tempdir;

  use super::open_log_file;

  #[test]
  fn debug_off_does_not_touch_the_log_path() {
    let root = tempdir().unwrap();
    let path = root.path().join("missing/log");

    assert!(open_log_file(false, &path).unwrap().is_none());
    assert!(!path.exists());
  }

  #[test]
  fn creates_and_appends_to_a_private_regular_file() {
    let root = tempdir().unwrap();
    let path = root.path().join("tuigreet.log");

    {
      let mut file = open_log_file(true, &path).unwrap().unwrap();
      writeln!(file, "first").unwrap();
    }
    {
      let mut file = open_log_file(true, &path).unwrap().unwrap();
      writeln!(file, "second").unwrap();
    }

    assert_eq!(fs::read_to_string(&path).unwrap(), "first\nsecond\n");
    assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
  }

  #[test]
  fn tightens_an_existing_log_file() {
    let root = tempdir().unwrap();
    let path = root.path().join("tuigreet.log");
    OpenOptions::new().write(true).create_new(true).open(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();

    drop(open_log_file(true, &path).unwrap());

    assert_eq!(fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o600);
  }

  #[test]
  fn refuses_a_symbolic_link_without_touching_its_target() {
    let root = tempdir().unwrap();
    let target = root.path().join("target");
    let link = root.path().join("tuigreet.log");
    fs::write(&target, "unchanged").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    assert!(open_log_file(true, &link).is_err());
    assert_eq!(fs::read_to_string(target).unwrap(), "unchanged");
  }

  #[test]
  fn refuses_a_fifo_without_blocking() {
    let root = tempdir().unwrap();
    let path = root.path().join("tuigreet.log");
    let path = CString::new(path.as_os_str().as_bytes()).unwrap();

    // SAFETY: `path` is a valid, NUL-terminated pathname and remains alive for
    // the duration of the call.
    assert_eq!(unsafe { nix::libc::mkfifo(path.as_ptr(), 0o600) }, 0);

    assert!(open_log_file(true, root.path().join("tuigreet.log").as_path()).is_err());
  }

  #[test]
  fn refuses_a_multiply_linked_regular_file() {
    let root = tempdir().unwrap();
    let path = root.path().join("tuigreet.log");
    fs::write(&path, "unchanged").unwrap();
    fs::hard_link(&path, root.path().join("alias")).unwrap();

    assert!(open_log_file(true, &path).is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), "unchanged");
  }
}
