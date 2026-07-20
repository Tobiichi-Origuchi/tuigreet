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

trait TerminalCommands {
  fn enable_raw(&mut self) -> io::Result<()>;
  fn enter_screen(&mut self) -> io::Result<()>;
  fn disable_raw(&mut self) -> io::Result<()>;
  fn clear(&mut self) -> io::Result<()>;
  fn move_home(&mut self) -> io::Result<()>;
  fn show_cursor(&mut self) -> io::Result<()>;
  fn leave_screen(&mut self) -> io::Result<()>;
}

#[cfg(not(test))]
struct CrosstermCommands;

#[cfg(not(test))]
impl TerminalCommands for CrosstermCommands {
  fn enable_raw(&mut self) -> io::Result<()> {
    enable_raw_mode()
  }

  fn enter_screen(&mut self) -> io::Result<()> {
    execute!(io::stdout(), EnterAlternateScreen, Clear(ClearType::All), Hide)
  }

  fn disable_raw(&mut self) -> io::Result<()> {
    disable_raw_mode()
  }

  fn clear(&mut self) -> io::Result<()> {
    execute!(io::stdout(), Clear(ClearType::All))
  }

  fn move_home(&mut self) -> io::Result<()> {
    execute!(io::stdout(), MoveTo(0, 0))
  }

  fn show_cursor(&mut self) -> io::Result<()> {
    execute!(io::stdout(), Show)
  }

  fn leave_screen(&mut self) -> io::Result<()> {
    execute!(io::stdout(), LeaveAlternateScreen)
  }
}

fn initialize_terminal(commands: &mut impl TerminalCommands) -> io::Result<()> {
  commands.enable_raw()?;
  commands.enter_screen()
}

fn restore_terminal(commands: &mut impl TerminalCommands) -> io::Result<()> {
  // Run every restoration step even if an earlier one fails. Disabling raw
  // mode first is the most important part for leaving a usable TTY behind.
  let mut first_error = None;
  record_error(&mut first_error, commands.disable_raw());
  record_error(&mut first_error, commands.clear());
  record_error(&mut first_error, commands.move_home());
  record_error(&mut first_error, commands.show_cursor());
  record_error(&mut first_error, commands.leave_screen());
  first_error.map_or(Ok(()), Err)
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

    let mut commands = CrosstermCommands;
    let guard = SessionGuard::enter(&TERMINAL, || initialize_terminal(&mut commands))?;

    Ok(Self { _guard: guard })
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
  fn enter(restorer: &R, setup: impl FnOnce() -> io::Result<()>) -> io::Result<SessionGuard<'_, R>> {
    let mut guard = SessionGuard { restorer, armed: true };
    if let Err(error) = setup() {
      let _ = guard.restore();
      return Err(error);
    }
    Ok(guard)
  }

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
      restore_terminal(&mut CrosstermCommands)
    }
  }
}

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
  use std::{
    cell::RefCell,
    sync::atomic::{AtomicUsize, Ordering},
  };

  use super::*;

  struct CountingRestorer {
    calls: AtomicUsize,
    fail: bool,
  }

  impl Restorer for CountingRestorer {
    fn restore(&self) -> io::Result<()> {
      self.calls.fetch_add(1, Ordering::Relaxed);
      if self.fail {
        Err(io::Error::other("injected restoration failure"))
      } else {
        Ok(())
      }
    }
  }

  struct RecordingCommands {
    calls: RefCell<Vec<&'static str>>,
    failures: Vec<&'static str>,
  }

  impl RecordingCommands {
    fn step(&self, name: &'static str) -> io::Result<()> {
      self.calls.borrow_mut().push(name);
      if self.failures.contains(&name) {
        Err(io::Error::other(format!("injected {name} failure")))
      } else {
        Ok(())
      }
    }
  }

  impl TerminalCommands for RecordingCommands {
    fn enable_raw(&mut self) -> io::Result<()> {
      self.step("enable_raw")
    }

    fn enter_screen(&mut self) -> io::Result<()> {
      self.step("enter_screen")
    }

    fn disable_raw(&mut self) -> io::Result<()> {
      self.step("disable_raw")
    }

    fn clear(&mut self) -> io::Result<()> {
      self.step("clear")
    }

    fn move_home(&mut self) -> io::Result<()> {
      self.step("move_home")
    }

    fn show_cursor(&mut self) -> io::Result<()> {
      self.step("show_cursor")
    }

    fn leave_screen(&mut self) -> io::Result<()> {
      self.step("leave_screen")
    }
  }

  #[test]
  fn explicit_restoration_and_drop_clean_up_exactly_once() {
    let restorer = CountingRestorer {
      calls: AtomicUsize::new(0),
      fail: false,
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
      fail: false,
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

  #[test]
  fn setup_failure_arms_cleanup_before_any_terminal_mutation() {
    let restorer = CountingRestorer {
      calls: AtomicUsize::new(0),
      fail: false,
    };
    let mut commands = RecordingCommands {
      calls: RefCell::new(Vec::new()),
      failures: vec!["enter_screen"],
    };

    let error = SessionGuard::enter(&restorer, || initialize_terminal(&mut commands))
      .err()
      .expect("injected setup failure was ignored");

    assert!(error.to_string().contains("enter_screen"));
    assert_eq!(*commands.calls.borrow(), ["enable_raw", "enter_screen"]);
    assert_eq!(restorer.calls.load(Ordering::Relaxed), 1);
  }

  #[test]
  fn restoration_runs_every_step_and_returns_the_first_failure() {
    let mut commands = RecordingCommands {
      calls: RefCell::new(Vec::new()),
      failures: vec!["disable_raw", "show_cursor"],
    };

    let error = restore_terminal(&mut commands).unwrap_err();

    assert!(error.to_string().contains("disable_raw"));
    assert_eq!(*commands.calls.borrow(), [
      "disable_raw",
      "clear",
      "move_home",
      "show_cursor",
      "leave_screen"
    ]);
  }

  #[test]
  fn explicit_restoration_failure_is_reported_once_without_drop_retry() {
    let restorer = CountingRestorer {
      calls: AtomicUsize::new(0),
      fail: true,
    };
    let mut session = SessionGuard {
      restorer: &restorer,
      armed: true,
    };

    assert!(session.restore().is_err());
    drop(session);
    assert_eq!(restorer.calls.load(Ordering::Relaxed), 1);
  }
}
