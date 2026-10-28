//! Cabinet controls for the A.S.T.R.O. pause menu, read straight from evdev.
//!
//! Every `/dev/input/event*` device is watched, so arcade encoders that show
//! up as keyboards (MAME layout), generic USB joysticks and gamepads all work.
//! While the menu is open the devices are grabbed (EVIOCGRAB), so RetroArch,
//! paused underneath, sees none of the menu presses: no stray hotkeys, and
//! ESC cannot quit the emulator.
//!
//! Menu key: TAB (MAME's menu key), the gamepad guide button, or START+SELECT
//! held together.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuInput {
    Toggle,
    Up,
    Down,
    Ok,
    Back,
}

const EV_KEY: u16 = 0x01;
const EV_ABS: u16 = 0x03;

const KEY_ESC: u16 = 1;
const KEY_BACKSPACE: u16 = 14;
const KEY_TAB: u16 = 15;
const KEY_ENTER: u16 = 28;
const KEY_LEFTCTRL: u16 = 29;
const KEY_LEFTALT: u16 = 56;
const KEY_UP: u16 = 103;
const KEY_DOWN: u16 = 108;
const BTN_TRIGGER: u16 = 0x120;
const BTN_THUMB: u16 = 0x121;
const BTN_BASE3: u16 = 0x128;
const BTN_BASE4: u16 = 0x129;
const BTN_SOUTH: u16 = 0x130;
const BTN_EAST: u16 = 0x131;
const BTN_SELECT: u16 = 0x13a;
const BTN_START: u16 = 0x13b;
const BTN_MODE: u16 = 0x13c;
const BTN_DPAD_UP: u16 = 0x220;
const BTN_DPAD_DOWN: u16 = 0x221;
pub const ABS_Y: u16 = 0x01;
const ABS_HAT0Y: u16 = 0x11;

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

/// Watch every input device (hotplug rescans every 5 s) and send menu inputs
/// to `tx`. `grab` asks the readers to take the devices exclusively.
#[cfg(target_os = "linux")]
pub fn spawn_readers(tx: tokio::sync::mpsc::UnboundedSender<MenuInput>, grab: Arc<AtomicBool>) {
    use std::collections::HashSet;
    use std::sync::Mutex;

    let open: Arc<Mutex<HashSet<std::path::PathBuf>>> = Arc::default();
    std::thread::spawn(move || loop {
        if let Ok(dir) = std::fs::read_dir("/dev/input") {
            for entry in dir.flatten() {
                let path = entry.path();
                let is_event = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("event"));
                if !is_event || !open.lock().unwrap().insert(path.clone()) {
                    continue;
                }
                let (tx, grab, open) = (tx.clone(), grab.clone(), open.clone());
                std::thread::spawn(move || {
                    if let Err(e) = read_device(&path, &tx, &grab) {
                        tracing::debug!("input {} closed: {e}", path.display());
                    }
                    open.lock().unwrap().remove(&path);
                });
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    });
}

#[cfg(not(target_os = "linux"))]
pub fn spawn_readers(_tx: tokio::sync::mpsc::UnboundedSender<MenuInput>, _grab: Arc<AtomicBool>) {}

#[cfg(target_os = "linux")]
fn read_device(
    path: &std::path::Path,
    tx: &tokio::sync::mpsc::UnboundedSender<MenuInput>,
    grab: &AtomicBool,
) -> std::io::Result<()> {
    use std::io::Read;
    use std::os::fd::AsRawFd;

    const EVIOCGRAB: libc::c_ulong = 0x4004_4590;
    // EVIOCGABS(ABS_Y): _IOR('E', 0x40 + ABS_Y, struct input_absinfo).
    const EVIOCGABS_Y: libc::c_ulong = 0x8018_4541;

    let mut file = std::fs::File::open(path)?;
    let fd = file.as_raw_fd();
    let mut absinfo = [0i32; 6];
    // SAFETY: fd is open for the call and absinfo is input_absinfo-sized.
    let y_range = (unsafe { libc::ioctl(fd, EVIOCGABS_Y, absinfo.as_mut_ptr()) } == 0
        && absinfo[2] > absinfo[1])
        .then_some((absinfo[1], absinfo[2]));
    let mut mapper = Mapper::new(y_range);
    let mut grabbed = false;
    // struct input_event on 64-bit: timeval (16) + type u16 + code u16 + value i32.
    let mut buf = [0u8; 24];
    loop {
        file.read_exact(&mut buf)?;
        let want = grab.load(Ordering::Relaxed);
        if want != grabbed {
            // SAFETY: fd is open; EVIOCGRAB takes an int by value.
            unsafe { libc::ioctl(fd, EVIOCGRAB, want as libc::c_int) };
            grabbed = want;
        }
        let ev_type = u16::from_ne_bytes([buf[16], buf[17]]);
        let code = u16::from_ne_bytes([buf[18], buf[19]]);
        let value = i32::from_ne_bytes([buf[20], buf[21], buf[22], buf[23]]);
        if let Some(input) = mapper.feed(ev_type, code, value) {
            if tx.send(input).is_err() {
                return Ok(());
            }
        }
    }
}

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
}
