# B.E.E.R.C.A.D.E. RK3588 U-Boot A/B Boot Script
# Compile with: mkimage -C none -A arm -T script -d boot.cmd boot.scr

setenv _need_save 0
if test "${bootlimit}" != "3"; then
    setenv bootlimit 3
    setenv _need_save 1
fi
if test -z "${boot_slot}"; then
    setenv boot_slot a
    setenv _need_save 1
fi
if test -z "${altbootcmd}"; then
    setenv altbootcmd 'if test "${boot_slot}" = "b"; then setenv boot_slot a; else setenv boot_slot b; fi; setenv upgrade_available 0; setenv bootcount 0; saveenv; reset'
    setenv _need_save 1
fi
if test "${_need_save}" = "1"; then
    saveenv
fi

if test "${boot_slot}" = "b"; then
    setenv root_part "PARTLABEL=ROOT_B"
else
    setenv boot_slot "a"
    setenv root_part "PARTLABEL=ROOT_A"
fi

if test -z "${devtype}"; then setenv devtype "mmc"; fi
if test -z "${devnum}"; then setenv devnum "0"; fi
if test -z "${distro_bootpart}"; then setenv distro_bootpart "1"; fi
setenv dev_part "${devtype} ${devnum}:${distro_bootpart}"

if test -z "${kernel_addr_r}"; then setenv kernel_addr_r "0x02000000"; fi
if test -z "${fdt_addr_r}"; then setenv fdt_addr_r "0x0a100000"; fi
if test -z "${fdtoverlay_addr_r}"; then setenv fdtoverlay_addr_r "0x0a200000"; fi
if test -z "${ramdisk_addr_r}"; then setenv ramdisk_addr_r "0x10000000"; fi

setenv slot_dir "/slot-${boot_slot}"
setenv bootargs "earlycon=uart8250,mmio32,0xfeb50000 console=tty1 console=ttyFIQ0,1500000 root=${root_part} rootfstype=squashfs ro rootwait quiet splash"

echo "B.E.E.R.C.A.D.E. booting slot ${boot_slot} (${root_part})..."

if load ${dev_part} ${kernel_addr_r} ${slot_dir}/Image; then
    if load ${dev_part} ${fdt_addr_r} ${slot_dir}/rk3588s-rock-5c.dtb; then
        fdt addr ${fdt_addr_r}
        fdt resize 8192
        if test -e ${dev_part} ${slot_dir}/overlays/rock5c-hdmi-audio.dtbo; then
            if load ${dev_part} ${fdtoverlay_addr_r} ${slot_dir}/overlays/rock5c-hdmi-audio.dtbo; then
                fdt apply ${fdtoverlay_addr_r}
            fi
        fi
        if load ${dev_part} ${ramdisk_addr_r} ${slot_dir}/initramfs.img; then
            setenv initrd_size ${filesize}
            booti ${kernel_addr_r} ${ramdisk_addr_r}:${initrd_size} ${fdt_addr_r}
        fi
    fi
fi

echo "ERROR: failed to boot slot ${boot_slot}, resetting for rollback..."
reset
