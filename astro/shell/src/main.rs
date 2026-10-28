//! A.S.T.R.O. customer screen.
//!
//! Draws whatever `astro-daemon` says over the shell socket (call to action,
//! "getting your games ready", the player's game list, time's up) and sends
//! back the game the player picked. It has no menus and nothing to configure.
//! Launching a game stops this process to free the display; the daemon starts
//! it again when the game exits.

mod ui;

use beercade_shared::shell::{ShellAction, ShellFrame, ShellView, SHELL_SOCKET};
use sdl2::controller::{Button, GameController};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::Color;
use sdl2::rect::Rect;
use sdl2::render::{Canvas, TextureCreator};
use sdl2::ttf::Font;
use sdl2::video::{Window, WindowContext};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};
use ui::{clock, Input, Ui};

const W: u32 = 1920;
const H: u32 = 1080;

struct Palette {
    bg: Color,
    panel: Color,
    accent: Color,
    accent2: Color,
    warn: Color,
    text: Color,
    dim: Color,
}

/// Venue theme colours, from the design canvas artboards.
fn palette(theme: &str) -> Palette {
    let rgb = |v: u32| Color::RGB((v >> 16) as u8, (v >> 8) as u8, v as u8);
    let (bg, panel, accent, accent2, warn, text, dim) = match theme {
        "vector" => (0x04060A, 0x0A0E14, 0x5CF2FF, 0xFF4FD8, 0xFFE45C, 0xDDF8FF, 0x6F8A96),
        "bigsky" => (0x0B1F3A, 0x12305A, 0x4FA3FF, 0xFFD23F, 0xFF7A59, 0xF4F8FF, 0x8FA6C4),
        "pinball" => (0x14060A, 0x2A0C12, 0xFFC94A, 0xFF3B5C, 0xFFE9A8, 0xFFF4DD, 0xB08A70),
        "classic" => (0x05070D, 0x0D1220, 0x00F0FF, 0xFF00C8, 0xFFE600, 0xE8F6FF, 0x6C7A92),
        _ => (0x1A1108, 0x2A1C0E, 0xF2A900, 0xD9531E, 0xFFE0A3, 0xFFF3DC, 0xA88B66),
    };
    Palette {
        bg: rgb(bg),
        panel: rgb(panel),
        accent: rgb(accent),
        accent2: rgb(accent2),
        warn: rgb(warn),
        text: rgb(text),
        dim: rgb(dim),
    }
}

/// First font that exists: venue font dropped on the image, then DejaVu.
fn font_path(kind: &str) -> Option<&'static str> {
    let candidates: &[&'static str] = if kind == "display" {
        &[
            "/usr/share/fonts/beercade/display.ttf",
            "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        ]
    } else {
        &[
            "/usr/share/fonts/beercade/data.ttf",
            "/usr/share/fonts/TTF/DejaVuSansMono-Bold.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf",
        ]
    };
    candidates.iter().copied().find(|p| std::path::Path::new(p).exists())
}

#[derive(Clone, Copy)]
enum Align {
    Left,
    Center,
    Right,
}

struct Painter<'a, 'ttf> {
    canvas: &'a mut Canvas<Window>,
    tc: &'a TextureCreator<WindowContext>,
    fonts: &'a Fonts<'ttf>,
}

struct Fonts<'ttf> {
    huge: Font<'ttf, 'static>,
    big: Font<'ttf, 'static>,
    mid: Font<'ttf, 'static>,
    small: Font<'ttf, 'static>,
}

impl Painter<'_, '_> {
    fn text(&mut self, font: &Font, s: &str, color: Color, x: i32, y: i32, align: Align) {
        if s.is_empty() {
            return;
        }
        let Ok(surface) = font.render(s).blended(color) else { return };
        let Ok(tex) = self.tc.create_texture_from_surface(&surface) else { return };
        let (w, h) = (surface.width(), surface.height());
        let x = match align {
            Align::Left => x,
            Align::Center => x - w as i32 / 2,
            Align::Right => x - w as i32,
        };
        let _ = self.canvas.copy(&tex, None, Rect::new(x, y, w, h));
    }

    fn fill(&mut self, color: Color, r: Rect) {
        self.canvas.set_draw_color(color);
        let _ = self.canvas.fill_rect(r);
    }

    fn frame_rect(&mut self, color: Color, r: Rect, thickness: u32) {
        self.canvas.set_draw_color(color);
        for i in 0..thickness as i32 {
            let _ = self.canvas.draw_rect(Rect::new(
                r.x() + i,
                r.y() + i,
                r.width().saturating_sub(2 * i as u32),
                r.height().saturating_sub(2 * i as u32),
            ));
        }
    }

    /// QR code of `data`, `size` px square, centred on (cx, cy).
    fn qr(&mut self, data: &str, fg: Color, bg: Color, cx: i32, cy: i32, size: u32) {
        let Ok(code) = qrcode::QrCode::new(data.as_bytes()) else { return };
        let n = code.width() as u32;
        let quiet = 2;
        let cell = size / (n + 2 * quiet);
        let total = cell * (n + 2 * quiet);
        let (x0, y0) = (cx - total as i32 / 2, cy - total as i32 / 2);
        self.fill(bg, Rect::new(x0, y0, total, total));
        self.canvas.set_draw_color(fg);
        for y in 0..n {
            for x in 0..n {
                if code[(x as usize, y as usize)] == qrcode::Color::Dark {
                    let _ = self.canvas.fill_rect(Rect::new(
                        x0 + ((x + quiet) * cell) as i32,
                        y0 + ((y + quiet) * cell) as i32,
                        cell,
                        cell,
                    ));
                }
            }
        }
    }
}

fn draw(p: &mut Painter, ui: &Ui, now: Instant) {
    let Some(frame) = &ui.frame else {
        p.canvas.set_draw_color(Color::RGB(0, 0, 0));
        p.canvas.clear();
        return;
    };
    let c = palette(&frame.theme);
    let f = p.fonts;
    p.canvas.set_draw_color(c.bg);
    p.canvas.clear();
    let cx = W as i32 / 2;
    // Slow pulse for calls to action.
    let pulse = (now.elapsed().as_millis() / 600) % 2 == 0;
    p.text(&f.small, &frame.node_id, c.dim, 60, 40, Align::Left);

    match &frame.view {
        ShellView::Idle => {
            p.text(&f.huge, "READY PLAYER?", c.accent, cx, 300, Align::Center);
            let cta = if pulse { c.warn } else { c.text };
            p.text(&f.big, "SEE THE BAR TO PLAY", cta, cx, 520, Align::Center);
            p.text(&f.mid, "Buy a game-time pass at the B.O.S.S. kiosk", c.dim, cx, 660, Align::Center);
        }
        ShellView::Staging { session_code, ready, total, time_remaining_sec } => {
            p.text(&f.mid, &format!("TIME {}", clock(*time_remaining_sec)), c.warn, W as i32 - 60, 40, Align::Right);
            p.text(&f.big, "GETTING YOUR GAMES READY", c.accent, cx, 340, Align::Center);
            let bar = Rect::new(cx - 600, 520, 1200, 40);
            p.frame_rect(c.accent, bar, 3);
            if *total > 0 {
                let w = (1194 * *ready / *total).max(1);
                p.fill(c.accent2, Rect::new(bar.x() + 3, bar.y() + 3, w, 34));
            }
            p.text(&f.mid, &format!("{ready} OF {total} GAMES"), c.text, cx, 600, Align::Center);
            p.text(&f.small, &format!("SESSION CODE {session_code}"), c.dim, cx, 960, Align::Center);
        }
        ShellView::Picker { session_code, time_remaining_sec, games } if ui.changing => {
            let _ = games;
            p.text(&f.mid, &format!("TIME {}", clock(*time_remaining_sec)), c.warn, W as i32 - 60, 40, Align::Right);
            p.text(&f.big, "CHANGE YOUR GAMES", c.accent, cx, 150, Align::Center);
            p.text(&f.mid, "Go to the B.O.S.S. kiosk, choose \"I'm already playing\"", c.text, cx, 290, Align::Center);
            p.text(&f.mid, "and enter this code. Your clock keeps running.", c.text, cx, 350, Align::Center);
            p.text(&f.huge, session_code, c.warn, cx - 300, 560, Align::Center);
            p.qr(session_code, Color::RGB(0, 0, 0), Color::RGB(255, 255, 255), cx + 420, 640, 360);
            p.text(&f.small, "B BACK TO YOUR GAMES", c.dim, cx, 980, Align::Center);
        }
        ShellView::Picker { session_code, time_remaining_sec, games } => {
            let low = *time_remaining_sec <= 300;
            let tc = if low && pulse { c.accent2 } else { c.warn };
            p.text(&f.big, &format!("TIME {}", clock(*time_remaining_sec)), tc, W as i32 - 60, 30, Align::Right);
            p.text(&f.big, "PICK A GAME", c.accent, 60, 110, Align::Left);
            if games.is_empty() {
                p.text(&f.mid, "No games ready yet. Please ask the staff.", c.text, cx, 500, Align::Center);
            }
            const ROWS: usize = 7;
            let first = ui.sel.saturating_sub(ROWS - 1);
            for (row, (i, g)) in games.iter().enumerate().skip(first).take(ROWS).enumerate() {
                let y = 260 + row as i32 * 100;
                let r = Rect::new(60, y, W - 120, 88);
                let selected = i == ui.sel;
                if selected {
                    p.fill(c.panel, r);
                    p.frame_rect(c.accent, r, 3);
                    p.text(&f.mid, "\u{25B6}", c.accent, 90, y + 18, Align::Left);
                }
                let col = if selected { c.accent } else { c.text };
                p.text(&f.mid, &g.title, col, 150, y + 18, Align::Left);
                p.text(&f.small, &g.system.to_uppercase(), c.dim, W as i32 - 100, y + 28, Align::Right);
            }
            if games.len() > ROWS {
                p.text(&f.small, &format!("{} / {}", ui.sel + 1, games.len()), c.dim, cx, 965, Align::Center);
            }
            p.text(&f.small, &format!("CODE {session_code}"), c.dim, 60, 1010, Align::Left);
            p.text(
                &f.small,
                "\u{25B2}\u{25BC} SELECT  \u{00B7}  A PLAY  \u{00B7}  Y CHANGE MY GAMES",
                c.dim,
                W as i32 - 60,
                1010,
                Align::Right,
            );
        }
        ShellView::TimesUp { session_code, grace_left_sec } => {
            p.text(&f.huge, "TIME'S UP", c.accent, cx, 120, Align::Center);
            p.text(&f.huge, &grace_left_sec.to_string(), c.warn, cx - 420, 430, Align::Center);
            p.text(&f.mid, "SECONDS TO TOP UP", c.dim, cx - 420, 600, Align::Center);
            p.qr(session_code, Color::RGB(0, 0, 0), Color::RGB(255, 255, 255), cx + 380, 520, 320);
            p.text(&f.big, session_code, c.warn, cx + 380, 720, Align::Center);
            p.text(&f.mid, "Top up at the B.O.S.S. kiosk with this code", c.text, cx, 860, Align::Center);
            p.text(&f.small, "Your games are kept until the countdown ends", c.dim, cx, 940, Align::Center);
        }
        ShellView::Ended => {
            p.text(&f.huge, "THANKS FOR PLAYING", c.accent, cx, 380, Align::Center);
            p.text(&f.mid, "See you next round", c.dim, cx, 600, Align::Center);
        }
    }
}

/// Socket to the daemon, reconnecting as needed.
struct Link {
    stream: Option<(BufReader<UnixStream>, UnixStream)>,
    buf: String,
    retry_at: Instant,
}

impl Link {
    fn new() -> Self {
        Link { stream: None, buf: String::new(), retry_at: Instant::now() }
    }

    fn connect(&mut self) {
        if self.stream.is_some() || Instant::now() < self.retry_at {
            return;
        }
        match UnixStream::connect(SHELL_SOCKET) {
            Ok(s) => {
                let _ = s.set_nonblocking(true);
                match s.try_clone() {
                    Ok(w) => self.stream = Some((BufReader::new(s), w)),
                    Err(e) => eprintln!("astro-shell: socket clone failed: {e}"),
                }
            }
            Err(_) => self.retry_at = Instant::now() + Duration::from_millis(500),
        }
    }

    /// Latest frame received, if any.
    fn poll(&mut self) -> Option<ShellFrame> {
        self.connect();
        let (rd, _) = self.stream.as_mut()?;
        let mut latest = None;
        loop {
            match rd.read_line(&mut self.buf) {
                Ok(0) => {
                    self.stream = None;
                    break;
                }
                Ok(_) => {
                    if self.buf.ends_with('\n') {
                        match serde_json::from_str::<ShellFrame>(self.buf.trim()) {
                            Ok(f) => latest = Some(f),
                            Err(e) => eprintln!("astro-shell: bad frame: {e}"),
                        }
                        self.buf.clear();
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(_) => {
                    self.stream = None;
                    break;
                }
            }
        }
        latest
    }

    fn send(&mut self, a: &ShellAction) {
        let Some((_, wr)) = self.stream.as_mut() else { return };
        let Ok(mut line) = serde_json::to_string(a) else { return };
        line.push('\n');
        if wr.write_all(line.as_bytes()).is_err() {
            self.stream = None;
        }
    }
}

fn key_input(k: Keycode) -> Option<Input> {
    Some(match k {
        Keycode::Up | Keycode::W | Keycode::Kp8 => Input::Up,
        Keycode::Down | Keycode::S | Keycode::Kp2 => Input::Down,
        // MAME layout: button 1 is LCtrl, start 1 is "1".
        Keycode::Return | Keycode::KpEnter | Keycode::LCtrl | Keycode::Space | Keycode::Num1 => Input::Ok,
        Keycode::Escape | Keycode::Backspace | Keycode::LAlt => Input::Back,
        Keycode::Tab | Keycode::Y | Keycode::Num5 => Input::Change,
        _ => return None,
    })
}

fn button_input(b: Button) -> Option<Input> {
    Some(match b {
        Button::DPadUp => Input::Up,
        Button::DPadDown => Input::Down,
        Button::A | Button::Start => Input::Ok,
        Button::B => Input::Back,
        Button::Y | Button::Back => Input::Change,
        _ => return None,
    })
}

fn main() -> Result<(), String> {
    let sdl = sdl2::init()?;
    let video = sdl.video()?;
    let gc = sdl.game_controller()?;
    let ttf = sdl2::ttf::init().map_err(|e| e.to_string())?;
    sdl2::hint::set("SDL_RENDER_SCALE_QUALITY", "1");

    let window = video
        .window("A.S.T.R.O.", W, H)
        .fullscreen_desktop()
        .build()
        .map_err(|e| e.to_string())?;
    let mut canvas = window
        .into_canvas()
        .present_vsync()
        .build()
        .map_err(|e| e.to_string())?;
    canvas.set_logical_size(W, H).map_err(|e| e.to_string())?;
    sdl.mouse().show_cursor(false);
    let tc = canvas.texture_creator();

    let display = font_path("display").ok_or("no display font installed")?;
    let data = font_path("data").ok_or("no data font installed")?;
    let fonts = Fonts {
        huge: ttf.load_font(display, 150)?,
        big: ttf.load_font(display, 80)?,
        mid: ttf.load_font(data, 44)?,
        small: ttf.load_font(data, 30)?,
    };

    let mut controllers: Vec<GameController> = Vec::new();
    let mut events = sdl.event_pump()?;
    let mut link = Link::new();
    let mut ui = Ui::default();
    let started = Instant::now();
    // Axis navigation repeats at most this often.
    let mut last_axis = Instant::now();

    loop {
        for ev in events.poll_iter() {
            let input = match ev {
                // Closing is not a customer option; systemd owns our lifetime.
                Event::Quit { .. } => None,
                Event::ControllerDeviceAdded { which, .. } => {
                    if let Ok(c) = gc.open(which) {
                        controllers.push(c);
                    }
                    None
                }
                Event::KeyDown { keycode: Some(k), repeat: false, .. } => key_input(k),
                Event::ControllerButtonDown { button, .. } => button_input(button),
                Event::ControllerAxisMotion { axis: sdl2::controller::Axis::LeftY, value, .. }
                    if value.unsigned_abs() > 20_000 && last_axis.elapsed() > Duration::from_millis(220) =>
                {
                    last_axis = Instant::now();
                    Some(if value < 0 { Input::Up } else { Input::Down })
                }
                _ => None,
            };
            if let Some(action) = input.and_then(|i| ui.input(i)) {
                link.send(&action);
            }
        }
        if let Some(f) = link.poll() {
            ui.set_frame(f);
        }
        let mut painter = Painter { canvas: &mut canvas, tc: &tc, fonts: &fonts };
        draw(&mut painter, &ui, started);
        canvas.present();
        std::thread::sleep(Duration::from_millis(16));
    }
}
