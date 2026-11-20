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

#[test]
fn slot_device_uses_partlabel_across_all_platforms() {
    let platforms = [
        Platform::X86_64,
        Platform::Rk3588,
        Platform::Bcm2712,
        Platform::Bcm2711,
        Platform::GenericArm,
    ];
    for p in platforms {
        assert_eq!(p.slot_device(Slot::A), "/dev/disk/by-partlabel/ROOT_A");
        assert_eq!(p.slot_device(Slot::B), "/dev/disk/by-partlabel/ROOT_B");
    }
}

#[test]
fn active_slot_matches_by_partlabel_partlabel_and_legacy_sda() {
    for (cmdline, expected) in [
        ("root=/dev/disk/by-partlabel/ROOT_A quiet", Slot::A),
        ("root=/dev/disk/by-partlabel/ROOT_B quiet", Slot::B),
        ("root=PARTLABEL=ROOT_A ro", Slot::A),
        ("root=PARTLABEL=ROOT_B ro", Slot::B),
        ("root=/dev/sda2 ro", Slot::A),
        ("root=/dev/sda3 ro", Slot::B),
    ] {
        let root = FakeRoot::new();
        root.file("proc/cmdline", cmdline);
        assert_eq!(Platform::Rk3588.active_slot_at(&root.0), expected, "{cmdline}");
        assert_eq!(Platform::Bcm2712.active_slot_at(&root.0), expected, "{cmdline}");
        assert_eq!(Platform::X86_64.active_slot_at(&root.0), expected, "{cmdline}");
    }
}

#[test]
fn x86_64_commit_stages_plus_3_and_updates_loader_glob() {
    let root = FakeRoot::new();
    let slot_a = "title Beercade (slot a)\nlinux /beercade/slot-a/vmlinuz-linux\noptions root=PARTLABEL=ROOT_A\n";
    let slot_b = "title Beercade (slot b)\nlinux /beercade/slot-b/vmlinuz-linux\noptions root=PARTLABEL=ROOT_B\n";
    root.file("efi/loader/entries/beercade-slot-a.conf", slot_a)
        .file("efi/loader/entries/beercade-slot-b.conf", slot_b)
        .file(
            "efi/loader/loader.conf",
            "default beercade-slot-a*.conf\ntimeout 3\nconsole-mode keep\neditor no\n",
        );

    Platform::X86_64.commit_boot_slot_at(Slot::B, &root.0).unwrap();

    let entries = root.0.join("efi/loader/entries");
    assert!(entries.join("beercade-slot-a.conf").is_file());
    assert!(!entries.join("beercade-slot-b.conf").exists());
    assert_eq!(fs::read_to_string(entries.join("beercade-slot-b+3.conf")).unwrap(), slot_b);
    assert!(!entries.join("beercade-slot-b+3.conf.tmp").exists());

    let loader = fs::read_to_string(root.0.join("efi/loader/loader.conf")).unwrap();
    assert_eq!(
        loader,
        "default beercade-slot-b*.conf\ntimeout 3\nconsole-mode keep\neditor no\n"
    );
    assert!(!root.0.join("efi/loader/loader.conf.tmp").exists());

    // Re-committing Slot B when an exhausted +0-3 entry exists replaces it cleanly with +3.
    fs::rename(
        entries.join("beercade-slot-b+3.conf"),
        entries.join("beercade-slot-b+0-3.conf"),
    )
    .unwrap();
    Platform::X86_64.commit_boot_slot_at(Slot::B, &root.0).unwrap();
    assert!(!entries.join("beercade-slot-b+0-3.conf").exists());
    assert_eq!(fs::read_to_string(entries.join("beercade-slot-b+3.conf")).unwrap(), slot_b);
}

#[test]
fn x86_64_bless_boot_promotes_counted_entry_and_clears_other_exhausted() {
    let root = FakeRoot::new();
    let slot_a = "title Beercade (slot a)\nlinux /beercade/slot-a/vmlinuz-linux\n";
    let slot_b = "title Beercade (slot b)\nlinux /beercade/slot-b/vmlinuz-linux\n";
    root.file("efi/loader/entries/beercade-slot-a.conf", slot_a)
        .file("efi/loader/entries/beercade-slot-b+2-1.conf", slot_b)
        .file(
            "efi/loader/loader.conf",
            "default beercade-slot-b*.conf\ntimeout 3\nconsole-mode keep\neditor no\n",
        );

    Platform::X86_64.bless_boot_at(Slot::B, &root.0).unwrap();
    let entries = root.0.join("efi/loader/entries");
    assert!(!entries.join("beercade-slot-b+2-1.conf").exists());
    assert_eq!(fs::read_to_string(entries.join("beercade-slot-b.conf")).unwrap(), slot_b);

    // Simulate a failed Slot B upgrade (+0-3) that rolled back to Slot A.
    fs::rename(
        entries.join("beercade-slot-b.conf"),
        entries.join("beercade-slot-b+0-3.conf"),
    )
    .unwrap();
    Platform::X86_64.bless_boot_at(Slot::A, &root.0).unwrap();
    assert!(!entries.join("beercade-slot-b+0-3.conf").exists());
    assert_eq!(fs::read_to_string(entries.join("beercade-slot-b.conf")).unwrap(), slot_b);
    assert_eq!(
        fs::read_to_string(root.0.join("efi/loader/loader.conf")).unwrap(),
        "default beercade-slot-a*.conf\ntimeout 3\nconsole-mode keep\neditor no\n"
    );
}

#[test]
fn rpi_commit_writes_tryboot_and_bless_promotes_to_config() {
    for platform in [Platform::Bcm2711, Platform::Bcm2712] {
        let root = FakeRoot::new();
        let initial_cfg = "\
arm_64bit=1
initramfs initramfs.img followkernel

[pi4]
kernel=kernel8.img

[pi5]
kernel=kernel8.img

[all]
os_prefix=slot-a/
";
        root.file("boot/config.txt", initial_cfg);

        platform.commit_boot_slot_at(Slot::B, &root.0).unwrap();

        // config.txt must stay untouched for hardware one-shot rollback on power loss.
        assert_eq!(fs::read_to_string(root.0.join("boot/config.txt")).unwrap(), initial_cfg);
        let tryboot = fs::read_to_string(root.0.join("boot/tryboot.txt")).unwrap();
        assert!(tryboot.contains("os_prefix=slot-b/\n"));
        assert!(!tryboot.contains("os_prefix=slot-a/"));
        assert!(tryboot.contains("[pi4]\nkernel=kernel8.img\n"));
        assert!(tryboot.contains("[pi5]\nkernel=kernel8.img\n"));
        assert!(!root.0.join("boot/tryboot.txt.tmp").exists());

        // Successful boot on Slot B promotes os_prefix=slot-b/ into config.txt and removes tryboot.txt.
        platform.bless_boot_at(Slot::B, &root.0).unwrap();
        assert!(!root.0.join("boot/tryboot.txt").exists());
        let promoted = fs::read_to_string(root.0.join("boot/config.txt")).unwrap();
        assert!(promoted.contains("os_prefix=slot-b/\n"));
        assert!(!promoted.contains("os_prefix=slot-a/"));

        // Failed tryboot rolled back to Slot B keeps config.txt on Slot B and deletes stale tryboot.txt.
        platform.commit_boot_slot_at(Slot::A, &root.0).unwrap();
        assert!(root.0.join("boot/tryboot.txt").is_file());
        platform.bless_boot_at(Slot::B, &root.0).unwrap();
        assert!(!root.0.join("boot/tryboot.txt").exists());
        assert!(fs::read_to_string(root.0.join("boot/config.txt"))
            .unwrap()
            .contains("os_prefix=slot-b/\n"));
    }
}

#[test]
fn rpi_inserts_os_prefix_when_missing_from_config() {
    let root = FakeRoot::new();
    root.file("boot/config.txt", "arm_64bit=1\n[pi4]\nkernel=kernel8.img\n");

    Platform::Bcm2712.commit_boot_slot_at(Slot::B, &root.0).unwrap();
    let tryboot = fs::read_to_string(root.0.join("boot/tryboot.txt")).unwrap();
    assert_eq!(
        tryboot,
        "arm_64bit=1\nos_prefix=slot-b/\n[pi4]\nkernel=kernel8.img\n"
    );
}

#[test]
fn rk3588_commit_updates_extlinux_partlabel_and_legacy_atomically() {
    let root = FakeRoot::new();
    let cfg_a = "\
default beercade
label beercade
    kernel /Image
    append earlycon=uart8250,mmio32,0xfeb50000 root=PARTLABEL=ROOT_A rootfstype=squashfs ro rootwait
";
    root.file("boot/extlinux/extlinux.conf", cfg_a);

    Platform::Rk3588.commit_boot_slot_at(Slot::B, &root.0).unwrap();
    let out_b = fs::read_to_string(root.0.join("boot/extlinux/extlinux.conf")).unwrap();
    assert!(out_b.contains("root=PARTLABEL=ROOT_B"));
    assert!(!out_b.contains("root=PARTLABEL=ROOT_A"));
    assert!(!root.0.join("boot/extlinux/extlinux.conf.tmp").exists());

    Platform::Rk3588.commit_boot_slot_at(Slot::A, &root.0).unwrap();
    let out_a = fs::read_to_string(root.0.join("boot/extlinux/extlinux.conf")).unwrap();
    assert_eq!(out_a, cfg_a);

    // Legacy /dev/sda2 in extlinux.conf is also migrated to PARTLABEL=ROOT_B.
    root.file(
        "boot/extlinux/extlinux.conf",
        "label beercade\n  append root=/dev/sda2 rootwait\n",
    );
    Platform::Rk3588.commit_boot_slot_at(Slot::B, &root.0).unwrap();
    assert_eq!(
        fs::read_to_string(root.0.join("boot/extlinux/extlinux.conf")).unwrap(),
        "label beercade\n  append root=PARTLABEL=ROOT_B rootwait\n"
    );
}
