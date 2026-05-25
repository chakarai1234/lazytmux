use std::collections::{HashMap, HashSet};
use std::fmt;

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MetricsSnapshot {
    pub system: SystemMetrics,
    pub panes: HashMap<String, ProcessMetrics>,
    pub cpu_ready: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SystemMetrics {
    pub cpu_usage: Option<f32>,
    pub memory_used: u64,
    pub memory_total: u64,
    pub load_average: Option<LoadAverage>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoadAverage {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProcessMetrics {
    pub cpu_usage: f32,
    pub memory_bytes: u64,
    pub process_count: usize,
}

impl ProcessMetrics {
    pub fn add(&mut self, other: &Self) {
        self.cpu_usage += other.cpu_usage;
        self.memory_bytes = self.memory_bytes.saturating_add(other.memory_bytes);
        self.process_count = self.process_count.saturating_add(other.process_count);
    }
}

pub struct MetricsSampler {
    system: System,
    sample_count: usize,
}

impl MetricsSampler {
    pub fn new() -> Self {
        Self {
            system: System::new(),
            sample_count: 0,
        }
    }

    pub fn sample<'a, I>(&mut self, pane_pids: I) -> MetricsSnapshot
    where
        I: IntoIterator<Item = &'a str>,
    {
        if !sysinfo::IS_SUPPORTED_SYSTEM {
            return MetricsSnapshot::default();
        }

        self.system.refresh_memory();
        self.system.refresh_cpu_usage();
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_memory().with_cpu(),
        );
        self.sample_count = self.sample_count.saturating_add(1);

        let records = self.process_records();
        let panes = pane_pids
            .into_iter()
            .filter_map(|pid| {
                let root_pid = pid.parse::<u32>().ok()?;
                aggregate_process_tree(root_pid, &records).map(|metrics| (pid.to_string(), metrics))
            })
            .collect();

        let load = System::load_average();
        MetricsSnapshot {
            system: SystemMetrics {
                cpu_usage: Some(self.system.global_cpu_usage()),
                memory_used: self.system.used_memory(),
                memory_total: self.system.total_memory(),
                load_average: Some(LoadAverage {
                    one: load.one,
                    five: load.five,
                    fifteen: load.fifteen,
                }),
            },
            panes,
            cpu_ready: self.sample_count > 1,
        }
    }

    fn process_records(&self) -> Vec<ProcessRecord> {
        self.system
            .processes()
            .iter()
            .map(|(pid, process)| ProcessRecord {
                pid: pid.as_u32(),
                parent: process.parent().map(|parent| parent.as_u32()),
                cpu_usage: process.cpu_usage(),
                memory_bytes: process.memory(),
            })
            .collect()
    }
}

impl Default for MetricsSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for MetricsSampler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MetricsSampler")
            .field("sample_count", &self.sample_count)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ProcessRecord {
    pid: u32,
    parent: Option<u32>,
    cpu_usage: f32,
    memory_bytes: u64,
}

fn aggregate_process_tree(root_pid: u32, records: &[ProcessRecord]) -> Option<ProcessMetrics> {
    if !records.iter().any(|record| record.pid == root_pid) {
        return None;
    }

    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for record in records {
        if let Some(parent) = record.parent {
            children.entry(parent).or_default().push(record.pid);
        }
    }

    let mut metrics = ProcessMetrics::default();
    let mut visited = HashSet::new();
    let mut stack = vec![root_pid];
    while let Some(pid) = stack.pop() {
        if !visited.insert(pid) {
            continue;
        }

        if let Some(record) = records.iter().find(|record| record.pid == pid) {
            metrics.cpu_usage += record.cpu_usage;
            metrics.memory_bytes = metrics.memory_bytes.saturating_add(record.memory_bytes);
            metrics.process_count = metrics.process_count.saturating_add(1);
        }

        if let Some(child_pids) = children.get(&pid) {
            stack.extend(child_pids);
        }
    }

    Some(metrics)
}

pub fn format_cpu_usage(value: f32, ready: bool) -> String {
    if ready {
        format!("{value:.1}%")
    } else {
        "sampling".into()
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{bytes:.0} B")
    }
}

pub fn format_memory_usage(used: u64, total: u64) -> String {
    if total == 0 {
        return "unavailable".into();
    }

    let percent = (used as f64 / total as f64) * 100.0;
    format!(
        "{} / {} ({percent:.0}%)",
        format_bytes(used),
        format_bytes(total)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_process_tree_from_root_to_descendants() {
        let records = vec![
            ProcessRecord {
                pid: 10,
                parent: None,
                cpu_usage: 4.0,
                memory_bytes: 100,
            },
            ProcessRecord {
                pid: 11,
                parent: Some(10),
                cpu_usage: 2.5,
                memory_bytes: 50,
            },
            ProcessRecord {
                pid: 12,
                parent: Some(11),
                cpu_usage: 1.0,
                memory_bytes: 25,
            },
            ProcessRecord {
                pid: 20,
                parent: None,
                cpu_usage: 99.0,
                memory_bytes: 999,
            },
        ];

        let metrics = aggregate_process_tree(10, &records).unwrap();

        assert_eq!(metrics.cpu_usage, 7.5);
        assert_eq!(metrics.memory_bytes, 175);
        assert_eq!(metrics.process_count, 3);
    }

    #[test]
    fn returns_none_for_missing_root_process() {
        let records = vec![ProcessRecord {
            pid: 10,
            parent: None,
            cpu_usage: 4.0,
            memory_bytes: 100,
        }];

        assert_eq!(aggregate_process_tree(99, &records), None);
    }

    #[test]
    fn avoids_cycles_while_aggregating_process_tree() {
        let records = vec![
            ProcessRecord {
                pid: 10,
                parent: Some(12),
                cpu_usage: 1.0,
                memory_bytes: 10,
            },
            ProcessRecord {
                pid: 11,
                parent: Some(10),
                cpu_usage: 1.0,
                memory_bytes: 10,
            },
            ProcessRecord {
                pid: 12,
                parent: Some(11),
                cpu_usage: 1.0,
                memory_bytes: 10,
            },
        ];

        let metrics = aggregate_process_tree(10, &records).unwrap();

        assert_eq!(metrics.process_count, 3);
        assert_eq!(metrics.memory_bytes, 30);
    }
}
