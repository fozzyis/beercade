//! Hardware abstraction for B.E.E.R.C.A.D.E. nodes.
//!
//! The platform is detected at runtime from the device tree (ARM) or the
//! target architecture (x86_64), so one binary per architecture covers every
//! board of that architecture. Probes take a filesystem `root` so tests can
//! point them at a fake `/sys` + `/proc` tree; production code uses `/`.

use std::fmt;
use std::fs;
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
    pub fn slot_device(self, slot: Slot) -> &'static str {
        match (self, slot) {
            // x86 images address slots by GPT partition label so the disk can be
            // SATA, NVMe or virtio without changing anything.
            (Platform::X86_64, Slot::A) => "/dev/disk/by-partlabel/ROOT_A",
            (Platform::X86_64, Slot::B) => "/dev/disk/by-partlabel/ROOT_B",
            (_, Slot::A) => "/dev/sda2",
            (_, Slot::B) => "/dev/sda3",
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
        [(Slot::A, "PARTLABEL=ROOT_A"), (Slot::B, "PARTLABEL=ROOT_B")]
            .into_iter()
            .find(|&(slot, label)| dev == self.slot_device(slot) || dev == label)
            .map(|(slot, _)| slot)
    }

    /// Make the next boot load `slot`.
    pub fn commit_boot_slot(self, slot: Slot) -> Result<(), HalError> {
        match self {
            Platform::X86_64 => {
                let entry = match slot {
                    Slot::A => "beercade-slot-a.conf",
                    Slot::B => "beercade-slot-b.conf",
                };
                run(Command::new("bootctl").args(["set-default", entry]))
            }
            _ => self.commit_uboot(slot, Path::new("/boot/extlinux/extlinux.conf")),
        }
    }

    /// U-Boot: prefer the env var the boot script consults, else rewrite
    /// extlinux.conf atomically (write temp, then rename).
    fn commit_uboot(self, slot: Slot, extlinux: &Path) -> Result<(), HalError> {
        let name = match slot {
            Slot::A => "a",
            Slot::B => "b",
        };
        if run(Command::new("fw_setenv").args(["raid_slot_being_flashed", name])).is_ok() {
            return Ok(());
        }
        let config = fs::read_to_string(extlinux).map_err(|e| {
            HalError::Bootloader(format!("fw_setenv unavailable and {} unreadable: {e}", extlinux.display()))
        })?;
        let rewritten = point_extlinux_at(&config, self.slot_device(slot), self.slot_device(slot.other()));
        let tmp = extlinux.with_extension("conf.tmp");
        fs::write(&tmp, rewritten).map_err(|e| HalError::Io(tmp.clone(), e))?;
        fs::rename(&tmp, extlinux).map_err(|e| HalError::Io(extlinux.to_path_buf(), e))
    }
}

/// Rewrite every `root=<from>` in an extlinux config to `root=<to>`.
pub fn point_extlinux_at(config: &str, to: &str, from: &str) -> String {
    config.replace(&format!("root={from}"), &format!("root={to}"))
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
