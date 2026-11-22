//! Memory of the analysis tools.
//!
//! Each tool runs in its own process group. The resident memory of every
//! watched group, the sum over its processes (e.g. a Python adapter and the
//! JVM it starts), is sampled with `ps` by a single thread; the peak is kept
//! and groups above a limit are reported. A tool that ends before the first
//! sample has no measure.

use std::collections::HashMap;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

/// How often the memory is sampled.
const PERIOD: Duration = Duration::from_millis(250);

#[derive(Debug, Default)]
pub struct Memory {
    /// Peak resident memory of each watched process group, in KiB.
    peaks: Mutex<HashMap<u32, u64>>,
    /// Limit in KiB, above which a group is reported as exceeding it.
    limit: Option<u64>,
    stop: AtomicBool,
}

impl Memory {
    pub fn new(limit_mib: Option<u64>) -> Self {
        Memory {
            limit: limit_mib.map(|mib| mib * 1024),
            ..Memory::default()
        }
    }

    /// Samples the watched groups until `stop` is called.
    pub fn run(&self) {
        while !self.stop.load(Ordering::Relaxed) {
            self.sample();
            thread::sleep(PERIOD);
        }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn watch(&self, group: u32) {
        self.peaks().insert(group, 0);
    }

    /// Stops watching `group` and returns its peak, in KiB.
    pub fn forget(&self, group: u32) -> Option<u64> {
        self.peaks().remove(&group).filter(|&peak| peak > 0)
    }

    pub fn exceeded(&self, group: u32) -> bool {
        self.limit
            .is_some_and(|limit| self.peaks().get(&group).is_some_and(|&peak| peak > limit))
    }

    fn sample(&self) {
        if self.peaks().is_empty() {
            return;
        }
        let Ok(output) = Command::new("ps").args(["-A", "-o", "pgid=,rss="]).output() else {
            return;
        };
        let usage = usage_by_group(&String::from_utf8_lossy(&output.stdout));
        for (group, peak) in self.peaks().iter_mut() {
            if let Some(&now) = usage.get(group) {
                *peak = (*peak).max(now);
            }
        }
    }

    fn peaks(&self) -> std::sync::MutexGuard<'_, HashMap<u32, u64>> {
        self.peaks.lock().expect("poisoned lock")
    }
}

/// Resident memory in KiB of each process group, from `ps -o pgid=,rss=`.
fn usage_by_group(ps: &str) -> HashMap<u32, u64> {
    let mut usage = HashMap::new();
    for line in ps.lines() {
        let mut fields = line.split_whitespace().map(str::parse::<u64>);
        if let (Some(Ok(group)), Some(Ok(rss))) = (fields.next(), fields.next())
            && let Ok(group) = u32::try_from(group)
        {
            *usage.entry(group).or_default() += rss;
        }
    }
    usage
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_the_processes_of_a_group() {
        let usage = usage_by_group("  12  100\n 12 50\n7 3\nbad line\n");
        assert_eq!(usage[&12], 150);
        assert_eq!(usage[&7], 3);
        assert_eq!(usage.len(), 2);
    }

    #[test]
    fn keeps_the_peak_and_checks_the_limit() {
        let memory = Memory::new(Some(1));
        memory.watch(42);
        assert!(!memory.exceeded(42));
        memory.peaks().insert(42, 2048);
        assert!(memory.exceeded(42));
        assert_eq!(memory.forget(42), Some(2048));
        assert_eq!(memory.forget(42), None);
    }
}
