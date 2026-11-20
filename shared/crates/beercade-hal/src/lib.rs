//! Hardware abstraction for B.E.E.R.C.A.D.E. nodes.
//!
//! The platform is detected at runtime from the device tree (ARM) or the
//! target architecture (x86_64), so one binary per architecture covers every
//! board of that architecture. Probes take a filesystem `root` so tests can
//! point them at a fake `/sys` + `/proc` tree; production code uses `/`.

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
pub enum HalError {
    Io(PathBuf, std::io::Error),
    Parse(String),
    NoSensor,
    Bootloader(String),
}

impl fmt::Display for HalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HalError::Io(p, e) => write!(f, "{}: {e}", p.display()),
            HalError::Parse(m) => write!(f, "parse error: {m}"),
            HalError::NoSensor => write!(f, "no usable temperature sensor found"),
            HalError::Bootloader(m) => write!(f, "bootloader update failed: {m}"),
        }
    }
}

impl std::error::Error for HalError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Radxa ROCK 5C / Orange Pi 5 Pro (O.R.I.O.N.)
    Rk3588,
    /// Raspberry Pi 5 (B.O.S.S. host)
    Bcm2712,
    /// Raspberry Pi 4 (B.O.S.S. host)
    Bcm2711,
    GenericArm,
    X86_64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    A,
    B,
}

impl Slot {
    pub fn other(self) -> Slot {
        match self {
            Slot::A => Slot::B,
            Slot::B => Slot::A,
        }
    }

    fn id(self) -> &'static str {
        match self {
            Slot::A => "a",
            Slot::B => "b",
        }
    }

    fn partlabel(self) -> &'static str {
        match self {
            Slot::A => "PARTLABEL=ROOT_A",
            Slot::B => "PARTLABEL=ROOT_B",
        }
    }

    fn legacy_device(self) -> &'static str {
        match self {
            Slot::A => "/dev/sda2",
            Slot::B => "/dev/sda3",
        }
    }

    fn systemd_entry_prefix(self) -> &'static str {
        match self {
            Slot::A => "beercade-slot-a",
            Slot::B => "beercade-slot-b",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thermals {
    pub cpu_c: f32,
    /// `None` when the board exposes no separate GPU sensor.
    pub gpu_c: Option<f32>,
}

impl Platform {
    pub fn detect() -> Platform {
        Platform::detect_at(Path::new("/"))
    }

    pub fn detect_at(root: &Path) -> Platform {
        if cfg!(target_arch = "x86_64") {
            return Platform::X86_64;
        }
        Platform::from_compatible(&fs::read(root.join("proc/device-tree/compatible")).unwrap_or_default())
    }

    /// Classify a device-tree `compatible` blob (NUL-separated, board first).
    pub fn from_compatible(compat: &[u8]) -> Platform {
        let compat = String::from_utf8_lossy(compat);
        if compat.contains("rockchip,rk3588") {
            Platform::Rk3588
        } else if compat.contains("brcm,bcm2712") {
            Platform::Bcm2712
        } else if compat.contains("brcm,bcm2711") {
            Platform::Bcm2711
        } else {
            Platform::GenericArm
        }
    }

    /// (hardware, soc) strings reported in telemetry.
    pub fn hardware_info(self) -> (&'static str, &'static str) {
        match self {
            Platform::Rk3588 => ("Radxa Rock 5C", "Rockchip RK3588S (4x A76 + 4x A55)"),
            Platform::Bcm2712 => ("Raspberry Pi 5", "Broadcom BCM2712 (4x A76)"),
            Platform::Bcm2711 => ("Raspberry Pi 4", "Broadcom BCM2711 (4x A72)"),
            Platform::GenericArm => ("Generic ARM64 board", "AArch64"),
            Platform::X86_64 => ("x86_64 PC", "x86_64 (Intel/AMD)"),
        }
    }

    pub fn read_thermals(self) -> Result<Thermals, HalError> {
        self.read_thermals_at(Path::new("/"))
    }

    pub fn read_thermals_at(self, root: &Path) -> Result<Thermals, HalError> {
        match self {
            Platform::X86_64 => {
                let cpu = hwmon_temp(root, &["coretemp", "k10temp", "zenpower"])
                    .ok_or(HalError::NoSensor)?;
                let gpu = hwmon_temp(root, &["amdgpu", "i915", "xe", "nouveau"]);
                Ok(Thermals { cpu_c: cpu, gpu_c: gpu })
            }
            _ => {
                // ARM boards expose the SoC as zone0; RK3588 also has the GPU on zone1.
                let cpu = read_millicelsius(&root.join("sys/class/thermal/thermal_zone0/temp"))?;
                let gpu = read_millicelsius(&root.join("sys/class/thermal/thermal_zone1/temp")).ok();
                Ok(Thermals { cpu_c: cpu, gpu_c: gpu })
            }
        }
    }

    /// Cores to pin latency-critical emulation threads to, or `None` to leave
    /// scheduling alone.
    pub fn realtime_cpus(self, online_cpus: usize) -> Option<Vec<usize>> {
        match self {
            // Cortex-A76 cluster on RK3588 is cpu4..cpu7.
            Platform::Rk3588 if online_cpus >= 8 => Some((4..8).collect()),
            // Keep cpu0-1 for IRQs and housekeeping.
            Platform::X86_64 if online_cpus >= 4 => Some((2..online_cpus).collect()),
            _ => None,
        }
    }

    /// Block device for an A/B rootfs slot.
    ///
    /// All platforms address slots by GPT partition label so the underlying
    /// storage can be SD, eMMC, SATA, NVMe, or virtio without board-specific
    /// device paths.
    pub fn slot_device(self, slot: Slot) -> &'static str {
        match slot {
            Slot::A => "/dev/disk/by-partlabel/ROOT_A",
            Slot::B => "/dev/disk/by-partlabel/ROOT_B",
        }
    }

    pub fn active_slot(self) -> Slot {
        self.active_slot_at(Path::new("/"))
    }

    /// Which slot we booted from: kernel command line first, then the device
    /// mounted at `/`, falling back to A for dev environments.
    pub fn active_slot_at(self, root: &Path) -> Slot {
        let cmdline = fs::read_to_string(root.join("proc/cmdline")).unwrap_or_default();
        for arg in cmdline.split_whitespace() {
            if let Some(slot) = arg.strip_prefix("root=").and_then(|dev| self.slot_for_root(dev)) {
                return slot;
            }
        }
        let mounts = fs::read_to_string(root.join("proc/mounts")).unwrap_or_default();
        for line in mounts.lines() {
            let mut fields = line.split_whitespace();
            if let (Some(dev), Some("/")) = (fields.next(), fields.next()) {
                if let Some(slot) = self.slot_for_root(dev) {
                    return slot;
                }
            }
        }
        Slot::A
    }

    fn slot_for_root(self, dev: &str) -> Option<Slot> {
        [Slot::A, Slot::B].into_iter().find(|&slot| {
            dev == self.slot_device(slot)
                || dev == slot.partlabel()
                || dev == slot.legacy_device()
        })
    }

    /// Stage the next boot to load `slot` (with rollback protection on the live system).
    pub fn commit_boot_slot(self, slot: Slot) -> Result<(), HalError> {
        match self {
            Platform::X86_64 => {
                self.commit_boot_slot_at(slot, Path::new("/"))?;
                // Clear EFI LoaderEntryDefault so loader.conf's glob controls
                // fallback after 3 failed boots; ignore when EFI vars are absent.
                let _ = Command::new("bootctl").args(["set-default", ""]).output();
                Ok(())
            }
            Platform::Rk3588 => {
                let name = slot.id();
                if run(Command::new("fw_setenv").args(["boot_slot", name])).is_ok()
                    && run(Command::new("fw_setenv").args(["upgrade_available", "1"])).is_ok()
                    && run(Command::new("fw_setenv").args(["bootcount", "0"])).is_ok()
                {
                    return Ok(());
                }
                self.commit_boot_slot_at(slot, Path::new("/"))
            }
            Platform::Bcm2711 | Platform::Bcm2712 | Platform::GenericArm => {
                self.commit_boot_slot_at(slot, Path::new("/"))
            }
        }
    }

    /// Stage the next boot to load `slot` under `root` (filesystem-only, testable).
    pub fn commit_boot_slot_at(self, slot: Slot, root: &Path) -> Result<(), HalError> {
        match self {
            Platform::X86_64 => commit_systemd_boot_at(slot, root),
            Platform::Bcm2711 | Platform::Bcm2712 => commit_rpi_tryboot_at(slot, root),
            Platform::Rk3588 | Platform::GenericArm => update_extlinux_slot(self, slot, root),
        }
    }

    /// Mark the currently booted slot healthy so the bootloader will not roll back.
    pub fn bless_boot(self) -> Result<(), HalError> {
        match self {
            Platform::X86_64 => {
                let cmd_res = run(Command::new("systemd-bless-boot").arg("good"));
                let file_res = self.bless_boot_at(self.active_slot(), Path::new("/"));
                match (cmd_res, file_res) {
                    (_, Ok(())) | (Ok(()), Err(_)) => Ok(()),
                    (Err(e), Err(_)) => Err(e),
                }
            }
            Platform::Rk3588 => {
                run(Command::new("fw_setenv").args(["upgrade_available", "0"]))?;
                run(Command::new("fw_setenv").args(["bootcount", "0"]))
            }
            Platform::Bcm2711 | Platform::Bcm2712 | Platform::GenericArm => {
                self.bless_boot_at(self.active_slot(), Path::new("/"))
            }
        }
    }

    /// Promote/clean up bootloader files under `root` for `active` slot.
    pub fn bless_boot_at(self, active: Slot, root: &Path) -> Result<(), HalError> {
        match self {
            Platform::X86_64 => bless_systemd_boot_at(active, root),
            Platform::Bcm2711 | Platform::Bcm2712 => bless_rpi_boot_at(active, root),
            Platform::Rk3588 | Platform::GenericArm => {
                let extlinux = root.join("boot/extlinux/extlinux.conf");
                if extlinux.exists() {
                    update_extlinux_slot(self, active, root)?;
                }
                Ok(())
            }
        }
    }
}

/// Rewrite every `root=<from>` in an extlinux config to `root=<to>`.
pub fn point_extlinux_at(config: &str, to: &str, from: &str) -> String {
    config.replace(&format!("root={from}"), &format!("root={to}"))
}

fn write_file_atomic(path: &Path, contents: &[u8]) -> Result<(), HalError> {
    let mut tmp_os = path.as_os_str().to_os_string();
    tmp_os.push(".tmp");
    let tmp = PathBuf::from(tmp_os);
    let mut f = fs::File::create(&tmp).map_err(|e| HalError::Io(tmp.clone(), e))?;
    if let Err(e) = f.write_all(contents).and_then(|()| f.sync_all()) {
        drop(f);
        let _ = fs::remove_file(&tmp);
        return Err(HalError::Io(tmp, e));
    }
    drop(f);
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(HalError::Io(path.to_path_buf(), e));
    }
    if let Some(parent) = path.parent() {
        if let Ok(dir) = fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

fn sorted_conf_entries(entries_dir: &Path) -> Result<Vec<PathBuf>, HalError> {
    let rd = fs::read_dir(entries_dir).map_err(|e| HalError::Io(entries_dir.to_path_buf(), e))?;
    let mut paths: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(".conf"))
        })
        .collect();
    paths.sort();
    Ok(paths)
}

fn is_slot_counted_entry(name: &str, prefix: &str) -> bool {
    name.ends_with(".conf")
        && name
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('+'))
}

fn is_slot_exhausted_entry(name: &str, prefix: &str) -> bool {
    name.ends_with(".conf")
        && name
            .strip_prefix(prefix)
            .is_some_and(|rest| rest == "+0.conf" || rest.starts_with("+0-"))
}

fn set_systemd_boot_default(contents: &str, default_pattern: &str) -> String {
    let target = format!("default {default_pattern}");
    let mut replaced = false;
    let mut lines: Vec<String> = Vec::new();
    for line in contents.lines() {
        let trimmed = line.trim_start();
        let is_default = trimmed == "default"
            || trimmed.starts_with("default ")
            || trimmed.starts_with("default\t");
        if is_default {
            if !replaced {
                lines.push(target.clone());
                replaced = true;
            }
        } else {
            lines.push(line.to_string());
        }
    }
    if !replaced {
        lines.push(target);
    }
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

fn commit_systemd_boot_at(slot: Slot, root: &Path) -> Result<(), HalError> {
    let entries_dir = root.join("efi/loader/entries");
    let prefix = slot.systemd_entry_prefix();
    let clean_path = entries_dir.join(format!("{prefix}.conf"));
    let entries = sorted_conf_entries(&entries_dir)?;

    let template_path = if clean_path.is_file() {
        clean_path.clone()
    } else {
        entries
            .iter()
            .find(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| is_slot_counted_entry(n, prefix))
            })
            .cloned()
            .ok_or_else(|| {
                HalError::Bootloader(format!(
                    "no systemd-boot entry found for slot {} in {}",
                    slot.id(),
                    entries_dir.display()
                ))
            })?
    };

    let template =
        fs::read_to_string(&template_path).map_err(|e| HalError::Io(template_path.clone(), e))?;
    let staged_name = format!("{prefix}+3.conf");
    let staged_path = entries_dir.join(&staged_name);
    write_file_atomic(&staged_path, template.as_bytes())?;

    if clean_path.is_file() {
        fs::remove_file(&clean_path).map_err(|e| HalError::Io(clean_path.clone(), e))?;
    }
    for path in entries {
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if is_slot_counted_entry(name, prefix) && name != staged_name {
                fs::remove_file(&path).map_err(|e| HalError::Io(path.clone(), e))?;
            }
        }
    }

    let loader_conf = root.join("efi/loader/loader.conf");
    let loader_contents =
        fs::read_to_string(&loader_conf).map_err(|e| HalError::Io(loader_conf.clone(), e))?;
    let updated = set_systemd_boot_default(&loader_contents, &format!("{prefix}*.conf"));
    write_file_atomic(&loader_conf, updated.as_bytes())
}

fn bless_systemd_boot_at(active: Slot, root: &Path) -> Result<(), HalError> {
    let entries_dir = root.join("efi/loader/entries");
    let active_prefix = active.systemd_entry_prefix();
    let active_clean = entries_dir.join(format!("{active_prefix}.conf"));
    let entries = sorted_conf_entries(&entries_dir)?;

    let active_counted: Vec<PathBuf> = entries
        .iter()
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| is_slot_counted_entry(n, active_prefix))
        })
        .cloned()
        .collect();

    if let Some((first, rest)) = active_counted.split_first() {
        fs::rename(first, &active_clean).map_err(|e| HalError::Io(active_clean.clone(), e))?;
        for dup in rest {
            let _ = fs::remove_file(dup);
        }
    } else if !active_clean.is_file() {
        return Err(HalError::Bootloader(format!(
            "missing systemd-boot entry for active slot {} in {}",
            active.id(),
            entries_dir.display()
        )));
    }

    let other_prefix = active.other().systemd_entry_prefix();
    let other_clean = entries_dir.join(format!("{other_prefix}.conf"));
    let other_exhausted: Vec<PathBuf> = entries
        .iter()
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| is_slot_exhausted_entry(n, other_prefix))
        })
        .cloned()
        .collect();

    if let Some((first, rest)) = other_exhausted.split_first() {
        if !other_clean.is_file() {
            fs::rename(first, &other_clean).map_err(|e| HalError::Io(other_clean.clone(), e))?;
        } else {
            fs::remove_file(first).map_err(|e| HalError::Io(first.clone(), e))?;
        }
        for dup in rest {
            let _ = fs::remove_file(dup);
        }
    }

    let loader_conf = root.join("efi/loader/loader.conf");
    if loader_conf.is_file() {
        let loader_contents =
            fs::read_to_string(&loader_conf).map_err(|e| HalError::Io(loader_conf.clone(), e))?;
        let updated = set_systemd_boot_default(&loader_contents, &format!("{active_prefix}*.conf"));
        if updated != loader_contents {
            write_file_atomic(&loader_conf, updated.as_bytes())?;
        }
    }
    Ok(())
}

fn set_rpi_os_prefix(config: &str, slot: Slot) -> String {
    let target = format!("os_prefix=slot-{}/", slot.id());
    let mut replaced = false;
    let mut lines: Vec<String> = Vec::new();
    for line in config.lines() {
        let trimmed = line.trim();
        let is_os_prefix = !trimmed.starts_with('#')
            && trimmed
                .split_once('=')
                .is_some_and(|(k, _)| k.trim() == "os_prefix");
        if is_os_prefix {
            if !replaced {
                lines.push(target.clone());
                replaced = true;
            }
        } else {
            lines.push(line.to_string());
        }
    }
    if !replaced {
        if let Some(pos) = lines.iter().position(|l| l.trim() == "[all]") {
            lines.insert(pos + 1, target);
        } else if let Some(pos) = lines
            .iter()
            .position(|l| l.trim().starts_with('[') && l.trim().ends_with(']'))
        {
            lines.insert(pos, target);
        } else {
            lines.push(target);
        }
    }
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

fn commit_rpi_tryboot_at(slot: Slot, root: &Path) -> Result<(), HalError> {
    let boot_dir = root.join("boot");
    let config_path = boot_dir.join("config.txt");
    let tryboot_path = boot_dir.join("tryboot.txt");
    let config =
        fs::read_to_string(&config_path).map_err(|e| HalError::Io(config_path.clone(), e))?;
    let tryboot = set_rpi_os_prefix(&config, slot);
    write_file_atomic(&tryboot_path, tryboot.as_bytes())
}

fn bless_rpi_boot_at(active: Slot, root: &Path) -> Result<(), HalError> {
    let boot_dir = root.join("boot");
    let config_path = boot_dir.join("config.txt");
    let tryboot_path = boot_dir.join("tryboot.txt");
    let config =
        fs::read_to_string(&config_path).map_err(|e| HalError::Io(config_path.clone(), e))?;
    let updated = set_rpi_os_prefix(&config, active);
    if updated != config {
        write_file_atomic(&config_path, updated.as_bytes())?;
    }
    match fs::remove_file(&tryboot_path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(HalError::Io(tryboot_path, e)),
    }
}

fn update_extlinux_slot(platform: Platform, slot: Slot, root: &Path) -> Result<(), HalError> {
    let extlinux = root.join("boot/extlinux/extlinux.conf");
    let config = fs::read_to_string(&extlinux).map_err(|e| {
        HalError::Bootloader(format!("{} unreadable: {e}", extlinux.display()))
    })?;
    let target_part = slot.partlabel();
    let other = slot.other();
    let mut rewritten = point_extlinux_at(&config, target_part, other.partlabel());
    rewritten = point_extlinux_at(&rewritten, target_part, platform.slot_device(other));
    rewritten = point_extlinux_at(&rewritten, target_part, other.legacy_device());
    rewritten = point_extlinux_at(&rewritten, target_part, platform.slot_device(slot));
    rewritten = point_extlinux_at(&rewritten, target_part, slot.legacy_device());
    if rewritten != config {
        write_file_atomic(&extlinux, rewritten.as_bytes())?;
    }
    Ok(())
}

fn run(cmd: &mut Command) -> Result<(), HalError> {
    let status = cmd.status().map_err(|e| HalError::Bootloader(format!("{cmd:?}: {e}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(HalError::Bootloader(format!("{cmd:?} exited with {status}")))
    }
}

fn read_millicelsius(path: &Path) -> Result<f32, HalError> {
    let raw = fs::read_to_string(path).map_err(|e| HalError::Io(path.to_path_buf(), e))?;
    let mc = raw
        .trim()
        .parse::<f32>()
        .map_err(|e| HalError::Parse(format!("{}: {e}", path.display())))?;
    Ok(mc / 1000.0)
}

fn hwmon_temp(root: &Path, drivers: &[&str]) -> Option<f32> {
    let mut entries: Vec<_> = fs::read_dir(root.join("sys/class/hwmon")).ok()?.flatten().collect();
    // read_dir order is unspecified; sort so the result is deterministic.
    entries.sort_by_key(|e| e.file_name());
    entries.iter().find_map(|entry| {
        let name = fs::read_to_string(entry.path().join("name")).ok()?;
        if !drivers.contains(&name.trim()) {
            return None;
        }
        read_millicelsius(&entry.path().join("temp1_input")).ok()
    })
}

#[cfg(test)]
mod tests;
