#[cfg(all(not(test), panic = "abort"))]
use std::sync::Once;
use std::{
  io,
  sync::atomic::{AtomicBool, Ordering},
};

#[cfg(not(test))]
use crossterm::{
  cursor::{Hide, MoveTo, Show},
  execute,
  terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

trait Restorer {
  fn restore(&self) -> io::Result<()>;
}

struct RealTerminal {
  active: AtomicBool,
}

static TERMINAL: RealTerminal = RealTerminal {
  active: AtomicBool::new(false),
};
#[cfg(all(not(test), panic = "abort"))]
static PANIC_HOOK: Once = Once::new();

/// Owns all process-global terminal modes changed by tuigreet.
///
/// Ratatui only restores cursor visibility when its own `Terminal` instance
/// hid the cursor. Raw mode and the alternate screen remain the application's
/// responsibility, so this guard deliberately outlives the Ratatui terminal.
pub(crate) struct TerminalSession {
  _guard: SessionGuard<'static, RealTerminal>,
}

struct SessionGuard<'a, R: Restorer> {
  restorer: &'a R,
  armed: bool,
}

impl TerminalSession {
  #[cfg(not(test))]
  pub(crate) fn enter() -> io::Result<Self> {
    #[cfg(panic = "abort")]
    install_panic_hook();

    if TERMINAL.active.swap(true, Ordering::AcqRel) {
      return Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "terminal session is already active",
      ));
    }

    let session = Self {
      _guard: SessionGuard {
        restorer: &TERMINAL,
        armed: true,
      },
    };

    if let Err(error) = enable_raw_mode() {
      let _ = TERMINAL.restore();
      return Err(error);
    }
    if let Err(error) = execute!(io::stdout(), EnterAlternateScreen, Clear(ClearType::All), Hide) {
      let _ = TERMINAL.restore();
      return Err(error);
    }

    Ok(session)
  }

  // Integration tests use an in-memory backend and must not mutate the test
  // process's real terminal.
  #[cfg(test)]
  pub(crate) fn enter() -> io::Result<Self> {
    Ok(Self {
      _guard: SessionGuard {
        restorer: &TERMINAL,
        armed: false,
      },
    })
  }
}

impl<R: Restorer> SessionGuard<'_, R> {
  fn restore(&mut self) -> io::Result<()> {
    if !self.armed {
      return Ok(());
    }
    self.armed = false;
    self.restorer.restore()
  }
}

impl<R: Restorer> Drop for SessionGuard<'_, R> {
  fn drop(&mut self) {
    let _ = self.restore();
  }
}

impl Restorer for RealTerminal {
  fn restore(&self) -> io::Result<()> {
    if !self.active.swap(false, Ordering::AcqRel) {
      return Ok(());
    }

    #[cfg(test)]
    return Ok(());

    #[cfg(not(test))]
    {
      // Run every restoration step even if an earlier write fails. Disabling
      // raw mode first follows Crossterm/Ratatui guidance and is the most
      // important part for leaving a usable TTY behind.
      let mut first_error = None;
      record_error(&mut first_error, disable_raw_mode());
      record_error(&mut first_error, execute!(io::stdout(), Clear(ClearType::All)));
      record_error(&mut first_error, execute!(io::stdout(), MoveTo(0, 0)));
      record_error(&mut first_error, execute!(io::stdout(), Show));
      record_error(&mut first_error, execute!(io::stdout(), LeaveAlternateScreen));

      first_error.map_or(Ok(()), Err)
    }
  }
}

#[cfg(not(test))]
fn record_error(first: &mut Option<io::Error>, result: io::Result<()>) {
  if let Err(error) = result
    && first.is_none()
  {
    *first = Some(error);
  }
}

// Unwinding builds restore through `SessionGuard::drop`. Only aborting builds
// need a hook because their destructors are intentionally skipped; a panic in
// such a build terminates the whole process, so process-global restoration is
// appropriate there too.
#[cfg(all(not(test), panic = "abort"))]
fn install_panic_hook() {
  PANIC_HOOK.call_once(|| {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
      let _ = TERMINAL.restore();
      previous(info);
    }));
  });
}

pub(crate) struct TerminationSignals {
  #[cfg(not(test))]
  interrupt: tokio::signal::unix::Signal,
  #[cfg(not(test))]
  terminate: tokio::signal::unix::Signal,
}

impl TerminationSignals {
  #[cfg(not(test))]
  pub(crate) fn new() -> io::Result<Self> {
    use tokio::signal::unix::{SignalKind, signal};

    Ok(Self {
      interrupt: signal(SignalKind::interrupt())?,
      terminate: signal(SignalKind::terminate())?,
    })
  }

  #[cfg(test)]
  pub(crate) fn new() -> io::Result<Self> {
    Ok(Self {})
  }

  #[cfg(not(test))]
  pub(crate) async fn recv(&mut self) -> &'static str {
    tokio::select! {
      _ = self.interrupt.recv() => "SIGINT",
      _ = self.terminate.recv() => "SIGTERM",
    }
  }

  #[cfg(test)]
  pub(crate) async fn recv(&mut self) -> &'static str {
    std::future::pending().await
  }
}

#[cfg(test)]
mod tests {
  use std::sync::atomic::{AtomicUsize, Ordering};

  use super::*;

  struct CountingRestorer {
    calls: AtomicUsize,
  }

  impl Restorer for CountingRestorer {
    fn restore(&self) -> io::Result<()> {
      self.calls.fetch_add(1, Ordering::Relaxed);
      Ok(())
    }
  }

  #[test]
  fn explicit_restoration_and_drop_clean_up_exactly_once() {
    let restorer = CountingRestorer {
      calls: AtomicUsize::new(0),
    };
    let mut session = SessionGuard {
      restorer: &restorer,
      armed: true,
    };

    session.restore().unwrap();
    session.restore().unwrap();
    drop(session);

    assert_eq!(restorer.calls.load(Ordering::Relaxed), 1);
  }

  #[test]
  fn unwinding_drop_restores_an_armed_session() {
    let restorer = CountingRestorer {
      calls: AtomicUsize::new(0),
    };

    let result = std::panic::catch_unwind(|| {
      let _session = SessionGuard {
        restorer: &restorer,
        armed: true,
      };
      panic!("simulated application panic");
    });

    assert!(result.is_err());
    assert_eq!(restorer.calls.load(Ordering::Relaxed), 1);
  }
}
