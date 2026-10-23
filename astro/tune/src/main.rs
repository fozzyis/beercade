//! astro-tune: boot-time latency tuning for A.S.T.R.O.
//!
//! 1. performance governor on every cpufreq policy
//! 2. systemd AllowedCPUs= drop-ins splitting realtime / housekeeping CPUs
//! 3. hold a PM QoS request (/dev/cpu_dma_latency = 0) for the process lifetime

use beercade_hal::Platform;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::process::Command;

/// Parse a kernel cpu list ("0-7", "0,2-3") into CPU ids.
fn parse_cpu_list(s: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for part in s.trim().split(',').filter(|p| !p.is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => {
                if let (Ok(a), Ok(b)) = (a.trim().parse::<usize>(), b.trim().parse::<usize>()) {
                    out.extend(a..=b);
                }
            }
            None => {
                if let Ok(n) = part.trim().parse::<usize>() {
                    out.push(n);
                }
            }
        }
    }
    out
}

/// Format CPU ids as a compact list: [2,3,4,5] -> "2-5", [2,4,5] -> "2,4-5".
fn format_cpu_list(cpus: &[usize]) -> String {
    let mut v = cpus.to_vec();
    v.sort_unstable();
    v.dedup();
    let mut parts = Vec::new();
    let mut i = 0;
    while i < v.len() {
        let start = v[i];
        let mut end = start;
        while i + 1 < v.len() && v[i + 1] == end + 1 {
            i += 1;
            end = v[i];
        }
        parts.push(if start == end {
            start.to_string()
        } else {
            format!("{start}-{end}")
        });
        i += 1;
    }
    parts.join(",")
}

fn online_cpus(root: &Path) -> Vec<usize> {
    if let Ok(s) = fs::read_to_string(root.join("sys/devices/system/cpu/online")) {
        let v = parse_cpu_list(&s);
        if !v.is_empty() {
            return v;
        }
    }
    let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    (0..n).collect()
}

fn write_sysfs(path: &Path, value: &str) {
    match fs::write(path, value) {
        Ok(()) => eprintln!("astro-tune: {} = {}", path.display(), value),
        Err(e) => eprintln!("astro-tune: cannot write {}: {e}", path.display()),
    }
}

fn set_cpufreq(root: &Path) {
    let dir = root.join("sys/devices/system/cpu/cpufreq");
    let Ok(entries) = fs::read_dir(&dir) else {
        eprintln!("astro-tune: no cpufreq, skipping governor");
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if !p
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("policy"))
        {
            continue;
        }
        if p.join("scaling_governor").exists() {
            write_sysfs(&p.join("scaling_governor"), "performance");
        }
        if p.join("energy_performance_preference").exists() {
            write_sysfs(&p.join("energy_performance_preference"), "performance");
        }
    }
}

/// (unit, AllowedCPUs) pairs: the RT slice gets the HAL's realtime set, the
/// housekeeping units the rest. `None` when the HAL names no realtime set.
fn cpu_plan(platform: Platform, online: &[usize]) -> Option<Vec<(&'static str, String)>> {
    let rt = platform.realtime_cpus(online.len())?;
    let house: Vec<usize> = online.iter().copied().filter(|c| !rt.contains(c)).collect();
    let mut plan = vec![("astro-rt.slice", format_cpu_list(&rt))];
    if !house.is_empty() {
        for unit in ["system.slice", "user.slice", "init.scope"] {
            plan.push((unit, format_cpu_list(&house)));
        }
    }
    Some(plan)
}

/// `set-property --runtime` applies to running slices and scopes at once and
/// is forgotten at reboot, where this service runs again.
fn partition_cpus(platform: Platform, online: &[usize]) -> bool {
    let Some(plan) = cpu_plan(platform, online) else {
        eprintln!("astro-tune: no realtime cpu set for this platform, leaving scheduling alone");
        return false;
    };
    for (unit, list) in plan {
        let prop = format!("AllowedCPUs={list}");
        match Command::new("systemctl").args(["set-property", "--runtime", unit, &prop]).status() {
            Ok(s) if s.success() => eprintln!("astro-tune: {unit} {prop}"),
            Ok(s) => eprintln!("astro-tune: set-property {unit} exited with {s}"),
            Err(e) => eprintln!("astro-tune: set-property {unit} failed: {e}"),
        }
    }
    true
}

fn hold_pm_qos(root: &Path) -> Option<File> {
    let path = root.join("dev/cpu_dma_latency");
    match File::options().write(true).open(&path) {
        Ok(mut f) => match f.write_all(&0i32.to_ne_bytes()) {
            Ok(()) => {
                eprintln!("astro-tune: holding PM QoS cpu_dma_latency=0");
                Some(f)
            }
            Err(e) => {
                eprintln!("astro-tune: PM QoS write failed: {e}");
                None
            }
        },
        Err(e) => {
            eprintln!("astro-tune: cannot open {}: {e}", path.display());
            None
        }
    }
}

fn run(root: &Path, platform: Platform) -> Option<File> {
    let online = online_cpus(root);
    eprintln!("astro-tune: {} online cpus, platform {:?}", online.len(), platform);
    set_cpufreq(root);
    partition_cpus(platform, &online);
    hold_pm_qos(root)
}

fn main() {
    let _qos = run(Path::new("/"), Platform::detect());
    // The PM QoS request lives as long as the fd stays open.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("astro-tune-{name}-{}", std::process::id()))
    }

    #[test]
    fn format_lists() {
        assert_eq!(format_cpu_list(&[2, 3, 4, 5]), "2-5");
        assert_eq!(format_cpu_list(&[2, 4, 5]), "2,4-5");
        assert_eq!(format_cpu_list(&[0]), "0");
        assert_eq!(format_cpu_list(&[]), "");
        assert_eq!(format_cpu_list(&[5, 3, 4, 0]), "0,3-5");
    }

    #[test]
    fn parse_lists() {
        assert_eq!(parse_cpu_list("0-7\n"), (0..8).collect::<Vec<_>>());
        assert_eq!(parse_cpu_list("0,2-3"), vec![0, 2, 3]);
        assert_eq!(parse_cpu_list(""), Vec::<usize>::new());
    }

    #[test]
    fn online_from_root() {
        let root = tmp("online");
        let d = root.join("sys/devices/system/cpu");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("online"), "0,2-3\n").unwrap();
        assert_eq!(online_cpus(&root), vec![0, 2, 3]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn plan_for_eight_cpus() {
        let online: Vec<usize> = (0..8).collect();
        let plan = cpu_plan(Platform::X86_64, &online).unwrap();
        assert_eq!(plan[0], ("astro-rt.slice", "2-7".to_string()));
        assert_eq!(
            &plan[1..],
            &[
                ("system.slice", "0-1".to_string()),
                ("user.slice", "0-1".to_string()),
                ("init.scope", "0-1".to_string()),
            ]
        );
    }

    #[test]
    fn no_plan_without_rt() {
        assert!(cpu_plan(Platform::X86_64, &[0, 1]).is_none());
    }
}
