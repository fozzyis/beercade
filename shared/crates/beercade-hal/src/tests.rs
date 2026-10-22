use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Throwaway fake filesystem root under the system temp dir.
struct FakeRoot(PathBuf);

impl FakeRoot {
    fn new() -> FakeRoot {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "beercade-hal-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        FakeRoot(dir)
    }

    fn file(&self, rel: &str, contents: &str) -> &FakeRoot {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
        self
    }
}

impl Drop for FakeRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn compatible_strings_map_to_platforms() {
    assert_eq!(Platform::from_compatible(b"radxa,rock-5c\0rockchip,rk3588s\0"), Platform::Rk3588);
    assert_eq!(Platform::from_compatible(b"xunlong,orangepi-5-pro\0rockchip,rk3588s\0"), Platform::Rk3588);
    assert_eq!(Platform::from_compatible(b"raspberrypi,5-model-b\0brcm,bcm2712\0"), Platform::Bcm2712);
    assert_eq!(Platform::from_compatible(b"raspberrypi,4-model-b\0brcm,bcm2711\0"), Platform::Bcm2711);
    assert_eq!(Platform::from_compatible(b""), Platform::GenericArm);
}

#[test]
fn arm_thermals_read_zones_and_tolerate_missing_gpu() {
    let root = FakeRoot::new();
    root.file("sys/class/thermal/thermal_zone0/temp", "51234\n");
    let t = Platform::Rk3588.read_thermals_at(&root.0).unwrap();
    assert!((t.cpu_c - 51.234).abs() < 1e-3);
    assert_eq!(t.gpu_c, None);

    root.file("sys/class/thermal/thermal_zone1/temp", "60000\n");
    assert_eq!(Platform::Rk3588.read_thermals_at(&root.0).unwrap().gpu_c, Some(60.0));
}

#[test]
fn missing_sensor_is_an_error_not_a_fake_reading() {
    let root = FakeRoot::new();
    assert!(Platform::Rk3588.read_thermals_at(&root.0).is_err());
    assert!(matches!(Platform::X86_64.read_thermals_at(&root.0), Err(HalError::NoSensor)));
}

#[test]
fn x86_thermals_pick_matching_hwmon_drivers() {
    let root = FakeRoot::new();
    root.file("sys/class/hwmon/hwmon0/name", "acpitz\n")
        .file("sys/class/hwmon/hwmon0/temp1_input", "99000\n")
        .file("sys/class/hwmon/hwmon1/name", "k10temp\n")
        .file("sys/class/hwmon/hwmon1/temp1_input", "47500\n")
        .file("sys/class/hwmon/hwmon2/name", "amdgpu\n")
        .file("sys/class/hwmon/hwmon2/temp1_input", "55000\n");
    let t = Platform::X86_64.read_thermals_at(&root.0).unwrap();
    assert_eq!(t.cpu_c, 47.5);
    assert_eq!(t.gpu_c, Some(55.0));
}

#[test]
fn realtime_cpus_never_name_cores_that_do_not_exist() {
    assert_eq!(Platform::Rk3588.realtime_cpus(8), Some(vec![4, 5, 6, 7]));
    assert_eq!(Platform::Rk3588.realtime_cpus(4), None);
    assert_eq!(Platform::Bcm2712.realtime_cpus(4), None);
    assert_eq!(Platform::X86_64.realtime_cpus(8), Some(vec![2, 3, 4, 5, 6, 7]));
    assert_eq!(Platform::X86_64.realtime_cpus(2), None);
}

#[test]
fn active_slot_from_cmdline_then_mounts() {
    let root = FakeRoot::new();
    root.file("proc/cmdline", "console=tty1 root=/dev/sda3 rootwait");
    assert_eq!(Platform::Rk3588.active_slot_at(&root.0), Slot::B);

    root.file("proc/cmdline", "console=tty1 root=PARTLABEL=ROOT_B quiet");
    assert_eq!(Platform::X86_64.active_slot_at(&root.0), Slot::B);

    // Unrecognised root= falls through to /proc/mounts.
    root.file("proc/cmdline", "root=LABEL=ROOT rw")
        .file("proc/mounts", "proc /proc proc rw 0 0\n/dev/sda3 / squashfs ro 0 0\n");
    assert_eq!(Platform::Rk3588.active_slot_at(&root.0), Slot::B);
}

#[test]
fn substrings_do_not_fake_a_slot_match() {
    // "p3" or "sda3" appearing elsewhere must not select slot B.
    let root = FakeRoot::new();
    root.file("proc/cmdline", "root=/dev/sda2 resume=/dev/sda3 splash p3");
    assert_eq!(Platform::Rk3588.active_slot_at(&root.0), Slot::A);
}

#[test]
fn unknown_environment_defaults_to_slot_a() {
    let root = FakeRoot::new();
    assert_eq!(Platform::Rk3588.active_slot_at(&root.0), Slot::A);
}

#[test]
fn extlinux_rewrite_only_touches_root_argument() {
    let cfg = "label astro\n  append root=/dev/sda2 rootwait console=ttyS2\n";
    let out = point_extlinux_at(cfg, "/dev/sda3", "/dev/sda2");
    assert_eq!(out, "label astro\n  append root=/dev/sda3 rootwait console=ttyS2\n");
}
