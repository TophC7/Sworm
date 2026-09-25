use std::collections::{HashMap, HashSet};
use sworm_protocol::app::AppRuntimeInfo;

/// Return package metadata, process memory, and aggregate process-tree CPU time.
pub fn read_runtime_info(name: String, version: String) -> AppRuntimeInfo {
    let status = std::fs::read_to_string("/proc/self/status").ok();
    AppRuntimeInfo {
        name,
        version,
        memory_bytes: status
            .as_deref()
            .and_then(|value| parse_proc_status_value(value, "VmRSS:"))
            .and_then(|kib| kib.checked_mul(1024)),
        app_cpu_time_ticks: read_process_tree_cpu_time_ticks(std::process::id()),
        system_cpu_time_ticks: std::fs::read_to_string("/proc/stat")
            .ok()
            .and_then(|value| parse_system_cpu_time_ticks(&value)),
        thread_count: status
            .as_deref()
            .and_then(|value| parse_proc_status_value(value, "Threads:"))
            .and_then(|value| u32::try_from(value).ok()),
        // Exclude the directory descriptor opened by read_dir itself.
        file_descriptor_count: std::fs::read_dir("/proc/self/fd")
            .ok()
            .map(|entries| entries.filter_map(Result::ok).count().saturating_sub(1) as u32),
    }
}

#[derive(Debug)]
struct ProcessStat {
    pid: u32,
    parent_pid: u32,
    cpu_time_ticks: u64,
}

fn read_process_tree_cpu_time_ticks(root_pid: u32) -> Option<u64> {
    let processes = std::fs::read_dir("/proc")
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let pid = entry.file_name().to_str()?.parse::<u32>().ok()?;
            let stat = std::fs::read_to_string(entry.path().join("stat")).ok()?;
            let parsed = parse_process_stat(&stat)?;
            (parsed.pid == pid).then_some(parsed)
        })
        .collect::<Vec<_>>();

    process_tree_cpu_time_ticks(root_pid, &processes)
}

fn process_tree_cpu_time_ticks(root_pid: u32, processes: &[ProcessStat]) -> Option<u64> {
    let by_pid = processes
        .iter()
        .map(|process| (process.pid, process))
        .collect::<HashMap<_, _>>();
    if !by_pid.contains_key(&root_pid) {
        return None;
    }

    let mut children = HashMap::<u32, Vec<u32>>::new();
    for process in processes {
        children
            .entry(process.parent_pid)
            .or_default()
            .push(process.pid);
    }

    let mut total = 0u64;
    let mut stack = vec![root_pid];
    let mut visited = HashSet::new();
    while let Some(pid) = stack.pop() {
        if !visited.insert(pid) {
            continue;
        }
        let process = by_pid.get(&pid)?;
        total = total.checked_add(process.cpu_time_ticks)?;
        if let Some(process_children) = children.get(&pid) {
            stack.extend(process_children);
        }
    }
    Some(total)
}

fn parse_process_stat(stat: &str) -> Option<ProcessStat> {
    let name_start = stat.find('(')?;
    let name_end = stat.rfind(')')?;
    let pid = stat[..name_start].trim().parse::<u32>().ok()?;
    let mut fields = stat[name_end + 1..].split_whitespace();
    let parent_pid = fields.nth(1)?.parse::<u32>().ok()?;
    fields.nth(8)?;
    let cpu_time_ticks = (0..4)
        .try_fold(0i128, |total, _| {
            let value = fields.next()?.parse::<i64>().ok()?;
            total.checked_add(i128::from(value))
        })
        .and_then(|total| u64::try_from(total).ok())?;

    Some(ProcessStat {
        pid,
        parent_pid,
        cpu_time_ticks,
    })
}

fn parse_system_cpu_time_ticks(stat: &str) -> Option<u64> {
    let mut fields = stat
        .lines()
        .find(|line| line.starts_with("cpu "))?
        .split_whitespace()
        .skip(1);
    (0..8).try_fold(0u64, |total, _| {
        total.checked_add(fields.next()?.parse::<u64>().ok()?)
    })
}

fn parse_proc_status_value(status: &str, key: &str) -> Option<u64> {
    status
        .lines()
        .find_map(|line| line.strip_prefix(key))
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::{
        parse_proc_status_value, parse_process_stat, parse_system_cpu_time_ticks,
        process_tree_cpu_time_ticks, ProcessStat,
    };

    #[test]
    fn parses_proc_status_counts_and_kib_values() {
        let status = "Name:\tsworm\nVmRSS:\t12345 kB\nThreads:\t8\n";

        assert_eq!(parse_proc_status_value(status, "VmRSS:"), Some(12_345));
        assert_eq!(parse_proc_status_value(status, "Threads:"), Some(8));
        assert_eq!(parse_proc_status_value(status, "Missing:"), None);
    }

    #[test]
    fn parses_process_cpu_time_with_spaces_and_parentheses_in_name() {
        let stat = "42 (sworm) worker) S 7 0 0 0 0 0 0 0 0 0 10 20 3 4 0 0 0 0 99\n";

        let process = parse_process_stat(stat).unwrap();

        assert_eq!(process.pid, 42);
        assert_eq!(process.parent_pid, 7);
        assert_eq!(process.cpu_time_ticks, 37);
    }

    #[test]
    fn sums_only_root_process_and_descendants() {
        let processes = [
            ProcessStat {
                pid: 1,
                parent_pid: 0,
                cpu_time_ticks: 10,
            },
            ProcessStat {
                pid: 2,
                parent_pid: 1,
                cpu_time_ticks: 20,
            },
            ProcessStat {
                pid: 3,
                parent_pid: 2,
                cpu_time_ticks: 30,
            },
            ProcessStat {
                pid: 4,
                parent_pid: 0,
                cpu_time_ticks: 100,
            },
        ];

        assert_eq!(process_tree_cpu_time_ticks(1, &processes), Some(60));
        assert_eq!(process_tree_cpu_time_ticks(99, &processes), None);
    }

    #[test]
    fn sums_machine_cpu_capacity_without_double_counting_guest_time() {
        let stat = "cpu  1 2 3 4 5 6 7 8 900 1000\ncpu0 1 2 3 4 5 6 7 8 900 1000\n";

        assert_eq!(parse_system_cpu_time_ticks(stat), Some(36));
    }
}
