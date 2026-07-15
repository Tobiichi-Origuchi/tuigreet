use std::{fs, path::PathBuf, time::Duration};

use tokio::sync::mpsc::Sender;

use crate::event::Event;

const POLL_INTERVAL: Duration = Duration::from_secs(1);
const SETTLE_DELAY: Duration = Duration::from_millis(100);

pub fn spawn(paths: Vec<PathBuf>, sender: Sender<Event>) {
  tokio::spawn(async move {
    let mut known = fingerprints(&paths);
    let mut interval = tokio::time::interval(POLL_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval.tick().await;

    loop {
      interval.tick().await;
      let current = fingerprints(&paths);
      if current == known {
        continue;
      }

      tokio::time::sleep(SETTLE_DELAY).await;
      known = fingerprints(&paths);
      if sender.send(Event::ReloadConfig).await.is_err() {
        break;
      }
    }
  });
}

fn fingerprints(paths: &[PathBuf]) -> Vec<Option<Vec<u8>>> {
  paths.iter().map(|path| fs::read(path).ok()).collect()
}
