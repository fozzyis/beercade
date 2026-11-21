//! Cabinet controls for the A.S.T.R.O. pause menu, bridged from `astro_rt`.
//!
//! `astro_rt` runs the dedicated 1000 Hz evdev producer inside RetroArch and
//! forwards menu hotkeys over a non-blocking Unix `SOCK_DGRAM` socket
//! (`/run/astro/menu.sock`), while reading a shared-memory atomic grab flag
//! (`/dev/shm/astro_rt_menu`) so RetroArch sees zero menu presses while the
//! pause menu is open.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[cfg(target_os = "linux")]
pub const MENU_SOCK_PATH: &str = "/run/astro/menu.sock";
#[cfg(target_os = "linux")]
pub const MENU_SHM_PATH: &str = "/dev/shm/astro_rt_menu";

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuInput {
    Toggle,
    Up,
    Down,
    Ok,
    Back,
}

impl MenuInput {
    #[cfg(target_os = "linux")]
    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Toggle),
            1 => Some(Self::Up),
            2 => Some(Self::Down),
            3 => Some(Self::Ok),
            4 => Some(Self::Back),
            _ => None,
        }
    }
}

#[cfg(test)]
mod evdev_map {
    use super::MenuInput;

    pub(super) const EV_KEY: u16 = 0x01;
    pub(super) const EV_ABS: u16 = 0x03;

    const KEY_ESC: u16 = 1;
    const KEY_BACKSPACE: u16 = 14;
    pub(super) const KEY_TAB: u16 = 15;
    const KEY_ENTER: u16 = 28;
    pub(super) const KEY_LEFTCTRL: u16 = 29;
    pub(super) const KEY_LEFTALT: u16 = 56;
    const KEY_UP: u16 = 103;
    pub(super) const KEY_DOWN: u16 = 108;
    const BTN_TRIGGER: u16 = 0x120;
    const BTN_THUMB: u16 = 0x121;
    const BTN_BASE3: u16 = 0x128;
    const BTN_BASE4: u16 = 0x129;
    const BTN_SOUTH: u16 = 0x130;
    const BTN_EAST: u16 = 0x131;
    pub(super) const BTN_SELECT: u16 = 0x13a;
    pub(super) const BTN_START: u16 = 0x13b;
    const BTN_MODE: u16 = 0x13c;
    const BTN_DPAD_UP: u16 = 0x220;
    const BTN_DPAD_DOWN: u16 = 0x221;
    pub const ABS_Y: u16 = 0x01;
    pub(super) const ABS_HAT0Y: u16 = 0x11;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Vertical {
        Up,
        Neutral,
        Down,
    }

    /// Turns one device's raw events into menu inputs.
    #[derive(Debug)]
    pub struct Mapper {
        /// ABS_Y range, when the device has one.
        y_range: Option<(i32, i32)>,
        y: Vertical,
        hat: Vertical,
        start: bool,
        select: bool,
        combo_fired: bool,
    }

    impl Mapper {
        pub fn new(y_range: Option<(i32, i32)>) -> Self {
            Self {
                y_range,
                y: Vertical::Neutral,
                hat: Vertical::Neutral,
                start: false,
                select: false,
                combo_fired: false,
            }
        }

        pub fn feed(&mut self, ev_type: u16, code: u16, value: i32) -> Option<MenuInput> {
            match ev_type {
                EV_KEY => self.key(code, value),
                EV_ABS => self.abs(code, value),
                _ => None,
            }
        }

        fn key(&mut self, code: u16, value: i32) -> Option<MenuInput> {
            let down = value != 0;
            match code {
                BTN_START | BTN_BASE4 => self.start = down,
                BTN_SELECT | BTN_BASE3 => self.select = down,
                _ => {}
            }
            if self.start && self.select {
                if !self.combo_fired {
                    self.combo_fired = true;
                    return Some(MenuInput::Toggle);
                }
                return None;
            }
            if !self.start && !self.select {
                self.combo_fired = false;
            }
            // Presses only; autorepeat (2) and release (0) are ignored.
            if value != 1 {
                return None;
            }
            match code {
                KEY_TAB | BTN_MODE => Some(MenuInput::Toggle),
                KEY_UP | BTN_DPAD_UP => Some(MenuInput::Up),
                KEY_DOWN | BTN_DPAD_DOWN => Some(MenuInput::Down),
                KEY_ENTER | KEY_LEFTCTRL | BTN_SOUTH | BTN_TRIGGER => Some(MenuInput::Ok),
                KEY_ESC | KEY_BACKSPACE | KEY_LEFTALT | BTN_EAST | BTN_THUMB => Some(MenuInput::Back),
                _ => None,
            }
        }

        fn abs(&mut self, code: u16, value: i32) -> Option<MenuInput> {
            let (dir, slot) = match code {
                ABS_HAT0Y => (value.signum(), &mut self.hat),
                ABS_Y => {
                    let (min, max) = self.y_range?;
                    let quarter = (max - min) / 4;
                    let dir = if value <= min + quarter {
                        -1
                    } else if value >= max - quarter {
                        1
                    } else {
                        0
                    };
                    (dir, &mut self.y)
                }
                _ => return None,
            };
            let next = match dir {
                -1 => Vertical::Up,
                1 => Vertical::Down,
                _ => Vertical::Neutral,
            };
            // Fire on leaving neutral only, like a key press.
            let fired = *slot == Vertical::Neutral && next != Vertical::Neutral;
            *slot = next;
            match (fired, next) {
                (true, Vertical::Up) => Some(MenuInput::Up),
                (true, Vertical::Down) => Some(MenuInput::Down),
                _ => None,
            }
        }
    }
}

#[cfg(test)]
pub use evdev_map::Mapper;
#[cfg(test)]
#[allow(unused_imports)]
pub use evdev_map::ABS_Y;
#[cfg(test)]
use evdev_map::{
    ABS_HAT0Y, BTN_SELECT, BTN_START, EV_ABS, EV_KEY, KEY_DOWN, KEY_LEFTALT, KEY_LEFTCTRL, KEY_TAB,
};

/// Update the menu grab flag immediately and sync `/dev/shm/astro_rt_menu`.
pub fn set_grab(grab: &AtomicBool, active: bool) {
    grab.store(active, Ordering::Release);
    #[cfg(target_os = "linux")]
    sync_grab_shm(std::path::Path::new(MENU_SHM_PATH), active);
}

#[cfg(target_os = "linux")]
fn sync_grab_shm(shm_path: &std::path::Path, active: bool) {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::atomic::AtomicU32;

    if let Some(parent) = shm_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o644)
        .open(shm_path)
    {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(path = %shm_path.display(), "Failed to open menu grab shm: {e}");
            return;
        }
    };
    if file.metadata().map(|m| m.len() < 4).unwrap_or(true) {
        if let Err(e) = file.set_len(4) {
            tracing::warn!(path = %shm_path.display(), "Failed to size menu grab shm: {e}");
            return;
        }
    }
    // SAFETY: `file` is open for read/write with length >= 4; `mmap` returns page-aligned memory.
    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            4,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            file.as_raw_fd(),
            0,
        )
    };
    if ptr == libc::MAP_FAILED {
        tracing::warn!(path = %shm_path.display(), "Failed to mmap menu grab shm");
        return;
    }
    // SAFETY: `ptr` points to 4 valid page-aligned bytes of shared memory.
    unsafe {
        let atomic = &*(ptr as *const AtomicU32);
        atomic.store(u32::from(active), Ordering::Release);
        libc::munmap(ptr, 4);
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn spawn_readers_at(
    sock_path: &std::path::Path,
    shm_path: &std::path::Path,
    tx: tokio::sync::mpsc::UnboundedSender<MenuInput>,
    grab: Arc<AtomicBool>,
) -> Option<tokio::task::JoinHandle<()>> {
    use std::os::unix::fs::PermissionsExt;

    sync_grab_shm(shm_path, grab.load(Ordering::Acquire));

    if let Some(dir) = sock_path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::remove_file(sock_path);
    let sock = match tokio::net::UnixDatagram::bind(sock_path) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(path = %sock_path.display(), "Menu bridge socket unavailable: {e}");
            return None;
        }
    };
    if let Err(e) = std::fs::set_permissions(sock_path, std::fs::Permissions::from_mode(0o660)) {
        tracing::warn!(path = %sock_path.display(), "Failed to chmod menu socket: {e}");
    }

    Some(tokio::spawn(async move {
        let mut buf = [0u8; 64];
        loop {
            match sock.recv(&mut buf).await {
                Ok(n) if n >= 1 => {
                    for &b in &buf[..n] {
                        if let Some(input) = MenuInput::from_code(b) {
                            if tx.send(input).is_err() {
                                return;
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("Menu bridge recv error: {e}");
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            }
        }
    }))
}

/// Listen for 1-byte menu datagrams from `astro_rt` and sync `/dev/shm/astro_rt_menu`.
#[cfg(target_os = "linux")]
pub fn spawn_readers(tx: tokio::sync::mpsc::UnboundedSender<MenuInput>, grab: Arc<AtomicBool>) {
    let _ = spawn_readers_at(
        std::path::Path::new(MENU_SOCK_PATH),
        std::path::Path::new(MENU_SHM_PATH),
        tx,
        grab,
    );
}

#[cfg(not(target_os = "linux"))]
pub fn spawn_readers(_tx: tokio::sync::mpsc::UnboundedSender<MenuInput>, _grab: Arc<AtomicBool>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_encoder_mame_layout() {
        let mut m = Mapper::new(None);
        assert_eq!(m.feed(EV_KEY, KEY_TAB, 1), Some(MenuInput::Toggle));
        assert_eq!(m.feed(EV_KEY, KEY_TAB, 0), None);
        assert_eq!(m.feed(EV_KEY, KEY_DOWN, 1), Some(MenuInput::Down));
        assert_eq!(m.feed(EV_KEY, KEY_DOWN, 2), None, "autorepeat ignored");
        assert_eq!(m.feed(EV_KEY, KEY_LEFTCTRL, 1), Some(MenuInput::Ok));
        assert_eq!(m.feed(EV_KEY, KEY_LEFTALT, 1), Some(MenuInput::Back));
    }

    #[test]
    fn start_select_combo_fires_once() {
        let mut m = Mapper::new(None);
        assert_eq!(m.feed(EV_KEY, BTN_START, 1), None);
        assert_eq!(m.feed(EV_KEY, BTN_SELECT, 1), Some(MenuInput::Toggle));
        assert_eq!(m.feed(EV_KEY, BTN_SELECT, 0), None);
        assert_eq!(m.feed(EV_KEY, BTN_SELECT, 1), None, "start still held");
        m.feed(EV_KEY, BTN_START, 0);
        m.feed(EV_KEY, BTN_SELECT, 0);
        m.feed(EV_KEY, BTN_START, 1);
        assert_eq!(m.feed(EV_KEY, BTN_SELECT, 1), Some(MenuInput::Toggle));
    }

    #[test]
    fn analog_stick_edges() {
        let mut m = Mapper::new(Some((0, 255)));
        assert_eq!(m.feed(EV_ABS, ABS_Y, 127), None);
        assert_eq!(m.feed(EV_ABS, ABS_Y, 0), Some(MenuInput::Up));
        assert_eq!(m.feed(EV_ABS, ABS_Y, 10), None, "held, no repeat");
        assert_eq!(m.feed(EV_ABS, ABS_Y, 128), None);
        assert_eq!(m.feed(EV_ABS, ABS_Y, 255), Some(MenuInput::Down));
        assert_eq!(m.feed(EV_ABS, ABS_HAT0Y, -1), Some(MenuInput::Up));
        assert_eq!(m.feed(EV_ABS, ABS_HAT0Y, 0), None);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn menu_bridge_datagram_and_shm_grab() {
        let dir = std::env::temp_dir().join(format!("astro-input-bridge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sock_path = dir.join("menu.sock");
        let shm_path = dir.join("astro_rt_menu");

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let grab = Arc::new(AtomicBool::new(false));
        let handle = spawn_readers_at(&sock_path, &shm_path, tx, grab.clone()).unwrap();

        assert_eq!(std::fs::read(&shm_path).unwrap(), 0u32.to_ne_bytes());

        let client = tokio::net::UnixDatagram::unbound().unwrap();
        client.send_to(&[0, 1, 2, 3, 4], &sock_path).await.unwrap();
        assert_eq!(rx.recv().await, Some(MenuInput::Toggle));
        assert_eq!(rx.recv().await, Some(MenuInput::Up));
        assert_eq!(rx.recv().await, Some(MenuInput::Down));
        assert_eq!(rx.recv().await, Some(MenuInput::Ok));
        assert_eq!(rx.recv().await, Some(MenuInput::Back));

        grab.store(true, Ordering::Release);
        sync_grab_shm(&shm_path, true);
        assert_eq!(std::fs::read(&shm_path).unwrap(), 1u32.to_ne_bytes());

        sync_grab_shm(&shm_path, false);
        assert_eq!(std::fs::read(&shm_path).unwrap(), 0u32.to_ne_bytes());

        handle.abort();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
