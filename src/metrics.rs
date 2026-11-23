use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

const HISTORY_CAPACITY: usize = 60;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MetricsSnapshot {
    pub system: SystemMetrics,
    pub panes: HashMap<String, ProcessMetrics>,
    pub cpu_ready: bool,
    pub cpu_history: Vec<u64>,
    pub mem_history: Vec<u64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SystemMetrics {
    pub cpu_usage: Option<f32>,
    pub memory_used: u64,
    pub memory_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub load_average: Option<LoadAverage>,
    pub cpu_cores: usize,
    pub uptime_secs: u64,
    pub host_name: String,
    pub os_label: String,
}

impl SystemMetrics {
    pub fn memory_percent(&self) -> f32 {
        if self.memory_total == 0 {
            0.0
        } else {
            ((self.memory_used as f64 / self.memory_total as f64) * 100.0) as f32
        }
    }
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
    cpu_history: VecDeque<u64>,
    mem_history: VecDeque<u64>,
    host_name: String,
    os_label: String,
}

impl MetricsSampler {
    pub fn new() -> Self {
        let host_name = System::host_name().unwrap_or_else(|| "localhost".into());
        let os_name = System::name().unwrap_or_else(|| std::env::consts::OS.into());
        let os_label = format!("{os_name}/{}", std::env::consts::ARCH);
        Self {
            system: System::new(),
            sample_count: 0,
            cpu_history: VecDeque::with_capacity(HISTORY_CAPACITY),
            mem_history: VecDeque::with_capacity(HISTORY_CAPACITY),
            host_name,
            os_label,
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
        let by_pid: HashMap<u32, ProcessRecord> =
            records.iter().copied().map(|rec| (rec.pid, rec)).collect();
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        for record in &records {
            if let Some(parent) = record.parent {
                children.entry(parent).or_default().push(record.pid);
            }
        }

        let panes = pane_pids
            .into_iter()
            .filter_map(|pid| {
                let root_pid = pid.parse::<u32>().ok()?;
                aggregate_process_tree_indexed(root_pid, &by_pid, &children)
                    .map(|metrics| (pid.to_string(), metrics))
            })
            .collect();

        let load = System::load_average();
        let cpu_usage = self.system.global_cpu_usage();
        let mem_used = self.system.used_memory();
        let mem_total = self.system.total_memory();
        let cpu_ready = self.sample_count > 1;

        if cpu_ready {
            push_history(
                &mut self.cpu_history,
                cpu_usage.clamp(0.0, 100.0).round() as u64,
            );
        }
        let mem_pct = if mem_total == 0 {
            0
        } else {
            ((mem_used as f64 / mem_total as f64) * 100.0)
                .clamp(0.0, 100.0)
                .round() as u64
        };
        push_history(&mut self.mem_history, mem_pct);

        let cpu_cores = self
            .system
            .cpus()
            .len()
            .max(System::physical_core_count().unwrap_or(0));

        MetricsSnapshot {
            system: SystemMetrics {
                cpu_usage: Some(cpu_usage),
                memory_used: mem_used,
                memory_total: mem_total,
                swap_used: self.system.used_swap(),
                swap_total: self.system.total_swap(),
                load_average: Some(LoadAverage {
                    one: load.one,
                    five: load.five,
                    fifteen: load.fifteen,
                }),
                cpu_cores,
                uptime_secs: System::uptime(),
                host_name: self.host_name.clone(),
                os_label: self.os_label.clone(),
            },
            panes,
            cpu_ready,
            cpu_history: self.cpu_history.iter().copied().collect(),
            mem_history: self.mem_history.iter().copied().collect(),
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

fn push_history(history: &mut VecDeque<u64>, value: u64) {
    if history.len() >= HISTORY_CAPACITY {
        history.pop_front();
    }
    history.push_back(value);
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

fn aggregate_process_tree_indexed(
    root_pid: u32,
    by_pid: &HashMap<u32, ProcessRecord>,
    children: &HashMap<u32, Vec<u32>>,
) -> Option<ProcessMetrics> {
    if !by_pid.contains_key(&root_pid) {
        return None;
    }

    let mut metrics = ProcessMetrics::default();
    let mut visited = HashSet::new();
    let mut stack = vec![root_pid];
    while let Some(pid) = stack.pop() {
        if !visited.insert(pid) {
            continue;
        }

        if let Some(record) = by_pid.get(&pid) {
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

#[cfg(test)]
fn aggregate_process_tree(root_pid: u32, records: &[ProcessRecord]) -> Option<ProcessMetrics> {
    let by_pid: HashMap<u32, ProcessRecord> =
        records.iter().copied().map(|rec| (rec.pid, rec)).collect();
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for record in records {
        if let Some(parent) = record.parent {
            children.entry(parent).or_default().push(record.pid);
        }
    }
    aggregate_process_tree_indexed(root_pid, &by_pid, &children)
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

pub fn format_bytes_compact(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.1}G", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.0}M", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.0}K", bytes / KIB)
    } else {
        format!("{bytes:.0}B")
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

pub fn format_uptime(secs: u64) -> String {
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let mins = (secs % 3_600) / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {mins}m")
    } else {
        format!("{mins}m")
    }
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

    #[test]
    fn formats_uptime_and_compact_bytes() {
        assert_eq!(format_uptime(90_060), "1d 1h");
        assert_eq!(format_uptime(3_900), "1h 5m");
        assert_eq!(format_uptime(300), "5m");
        assert_eq!(format_bytes_compact(2 * 1024 * 1024), "2M");
        assert_eq!(format_bytes_compact(3 * 1024 * 1024 * 1024), "3.0G");
    }
}
