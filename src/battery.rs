use std::{fs, path::Path, sync::Arc, time::Duration};

use tokio::{
  sync::{RwLock, mpsc::Sender, oneshot, watch},
  task::JoinHandle,
  time::{MissedTickBehavior, interval},
};

use crate::{Greeter, event::Event};

const POWER_SUPPLY_ROOT: &str = "/sys/class/power_supply";
const SAMPLE_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BatteryInfo {
  pub percentage: u8,
  pub charging: bool,
}

pub(crate) struct BatteryMonitor {
  enabled: watch::Sender<bool>,
  shutdown: Option<oneshot::Sender<()>>,
  worker: JoinHandle<()>,
}

impl BatteryMonitor {
  pub(crate) fn spawn(enabled: bool, greeter: Arc<RwLock<Greeter>>, renders: Sender<Event>) -> Self {
    let (enabled_tx, enabled_rx) = watch::channel(enabled);
    let (shutdown, shutdown_rx) = oneshot::channel();
    let worker = tokio::spawn(run_monitor(greeter, renders, enabled_rx, shutdown_rx));

    Self {
      enabled: enabled_tx,
      shutdown: Some(shutdown),
      worker,
    }
  }

  pub(crate) fn set_enabled(&self, enabled: bool) {
    self.enabled.send_if_modified(|current| {
      if *current == enabled {
        false
      } else {
        *current = enabled;
        true
      }
    });
  }

  pub(crate) async fn shutdown(&mut self) {
    if let Some(shutdown) = self.shutdown.take() {
      let _ = shutdown.send(());
    }
    if tokio::time::timeout(Duration::from_secs(1), &mut self.worker)
      .await
      .is_err()
    {
      self.worker.abort();
      let _ = (&mut self.worker).await;
    }
  }
}

impl Drop for BatteryMonitor {
  fn drop(&mut self) {
    if let Some(shutdown) = self.shutdown.take() {
      let _ = shutdown.send(());
    }
    self.worker.abort();
  }
}

async fn run_monitor(
  greeter: Arc<RwLock<Greeter>>,
  renders: Sender<Event>,
  mut enabled: watch::Receiver<bool>,
  mut shutdown: oneshot::Receiver<()>,
) {
  let mut sample_interval = interval(SAMPLE_INTERVAL);
  sample_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

  loop {
    tokio::select! {
      biased;
      _ = &mut shutdown => break,
      changed = enabled.changed() => {
        if changed.is_err() {
          break;
        }
      },
      _ = sample_interval.tick() => {},
    }

    let info = if *enabled.borrow() {
      tokio::task::spawn_blocking(|| read_battery_info(Path::new(POWER_SUPPLY_ROOT)))
        .await
        .unwrap_or(None)
    } else {
      None
    };
    let changed = {
      let mut state = greeter.write().await;
      if state.battery_info == info {
        false
      } else {
        state.battery_info = info;
        true
      }
    };
    if changed {
      let _ = renders.try_send(Event::Render);
    }
  }
}

fn read_battery_info(root: &Path) -> Option<BatteryInfo> {
  let mut entries = fs::read_dir(root).ok()?.flatten().collect::<Vec<_>>();
  entries.sort_by_key(|entry| entry.file_name());

  let mut samples = Vec::new();
  let mut charging = false;
  for entry in entries {
    let path = entry.path();
    let name_is_battery = entry.file_name().to_string_lossy().starts_with("BAT");
    let type_is_battery = read_trimmed(&path.join("type")).is_some_and(|kind| kind == "Battery");
    if !name_is_battery && !type_is_battery {
      continue;
    }

    let sample = raw_ratio(&path, "energy_now", "energy_full")
      .map(|(current, full)| BatterySample::Energy { current, full })
      .or_else(|| {
        raw_ratio(&path, "charge_now", "charge_full").map(|(current, full)| BatterySample::Charge { current, full })
      })
      .or_else(|| read_number(&path.join("capacity")).map(|value| BatterySample::Capacity(value.min(100) as u8)));
    if let Some(sample) = sample {
      samples.push(sample);
      charging |= read_trimmed(&path.join("status")).is_some_and(|status| status == "Charging");
    }
  }

  aggregate_percentage(&samples).map(|percentage| BatteryInfo { percentage, charging })
}

#[derive(Clone, Copy)]
enum BatterySample {
  Energy { current: u64, full: u64 },
  Charge { current: u64, full: u64 },
  Capacity(u8),
}

impl BatterySample {
  fn percentage(self) -> u8 {
    match self {
      Self::Energy { current, full } | Self::Charge { current, full } => percentage(current.into(), full.into()),
      Self::Capacity(value) => value,
    }
  }
}

fn aggregate_percentage(samples: &[BatterySample]) -> Option<u8> {
  if samples.is_empty() {
    return None;
  }

  let aggregate = |select: fn(BatterySample) -> Option<(u64, u64)>| {
    samples
      .iter()
      .copied()
      .map(select)
      .try_fold((0_u128, 0_u128), |(current, full), values| {
        values.map(|(next_current, next_full)| (current + u128::from(next_current), full + u128::from(next_full)))
      })
      .map(|(current, full)| percentage(current, full))
  };

  aggregate(|sample| match sample {
    BatterySample::Energy { current, full } => Some((current, full)),
    _ => None,
  })
  .or_else(|| {
    aggregate(|sample| match sample {
      BatterySample::Charge { current, full } => Some((current, full)),
      _ => None,
    })
  })
  .or_else(|| {
    let count = u64::try_from(samples.len()).ok()?;
    (count != 0).then(|| {
      let total = samples
        .iter()
        .copied()
        .map(BatterySample::percentage)
        .map(u64::from)
        .sum::<u64>();
      u8::try_from(total / count).unwrap_or(100)
    })
  })
}

fn raw_ratio(root: &Path, current: &str, full: &str) -> Option<(u64, u64)> {
  let current = read_number(&root.join(current))?;
  let full = read_number(&root.join(full))?;
  (full != 0).then_some((current, full))
}

fn percentage(current: u128, full: u128) -> u8 {
  let percentage = current.saturating_mul(100).checked_div(full).unwrap_or(0).min(100);
  u8::try_from(percentage).unwrap_or(100)
}

fn read_number(path: &Path) -> Option<u64> {
  read_trimmed(path)?.parse().ok()
}

fn read_trimmed(path: &Path) -> Option<String> {
  fs::read_to_string(path).ok().map(|value| value.trim().to_string())
}

#[cfg(test)]
mod tests {
  use std::fs;

  use tempfile::tempdir;

  use super::*;

  fn write_supply(root: &Path, name: &str, fields: &[(&str, &str)]) {
    let supply = root.join(name);
    fs::create_dir(&supply).unwrap();
    for (field, value) in fields {
      fs::write(supply.join(field), value).unwrap();
    }
  }

  #[test]
  fn samples_all_batteries_and_ignores_other_power_supplies() {
    let directory = tempdir().unwrap();
    write_supply(directory.path(), "BAT0", &[
      ("type", "Battery"),
      ("energy_now", "30"),
      ("energy_full", "60"),
      ("status", "Discharging"),
    ]);
    write_supply(directory.path(), "aux", &[
      ("type", "Battery"),
      ("capacity", "90"),
      ("status", "Charging"),
    ]);
    write_supply(directory.path(), "AC", &[("type", "Mains"), ("capacity", "1")]);

    assert_eq!(
      read_battery_info(directory.path()),
      Some(BatteryInfo {
        percentage: 70,
        charging: true,
      })
    );
  }

  #[test]
  fn compatible_batteries_are_weighted_by_their_full_capacity() {
    let directory = tempdir().unwrap();
    write_supply(directory.path(), "BAT0", &[
      ("type", "Battery"),
      ("energy_now", "30"),
      ("energy_full", "60"),
      ("status", "Discharging"),
    ]);
    write_supply(directory.path(), "BAT1", &[
      ("type", "Battery"),
      ("energy_now", "90"),
      ("energy_full", "100"),
      ("status", "Charging"),
    ]);

    assert_eq!(
      read_battery_info(directory.path()),
      Some(BatteryInfo {
        percentage: 75,
        charging: true,
      })
    );
  }

  #[test]
  fn incompatible_measurement_units_fall_back_to_per_battery_percentages() {
    let samples = [
      BatterySample::Energy { current: 30, full: 60 },
      BatterySample::Charge { current: 90, full: 100 },
      BatterySample::Capacity(70),
    ];
    assert_eq!(aggregate_percentage(&samples), Some(70));
  }

  #[test]
  fn malformed_or_empty_supplies_are_ignored_and_values_are_clamped() {
    let directory = tempdir().unwrap();
    write_supply(directory.path(), "BAT0", &[("capacity", "invalid")]);
    assert_eq!(read_battery_info(directory.path()), None);

    write_supply(directory.path(), "BAT1", &[("capacity", "150")]);
    assert_eq!(read_battery_info(directory.path()).unwrap().percentage, 100);
  }
}
