//! DOCKADE customer screen (A.S.T.R.O.).
//!
//! Resident KMS/SDL2 process driven by `astro-daemon` over `/run/astro/shell.sock`:
//! receives `ShellFrame` pushes (call to action, staging progress, game picker,
//! time's up, or `Suspended`) and sends back `ShellAction`s. On `Suspended`, it
//! drops its SDL2/KMSDRM display context, acknowledges `DisplayReleased`, and
//! waits for `SIGCONT` + the next active frame before reopening KMSDRM.

mod ui;

use beercade_shared::shell::{ShellAction, ShellFrame, ShellView};
#[cfg(unix)]
use beercade_shared::shell::SHELL_SOCKET;
use beercade_shared::theme::{palette as theme_palette, Rgb};
use sdl2::controller::{Button, GameController};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::rect::Rect;
use sdl2::render::{Canvas, Texture, TextureCreator};
use sdl2::surface::Surface;
use sdl2::ttf::Font;
use sdl2::video::{Window, WindowContext};
use sdl2::{EventPump, GameControllerSubsystem, Sdl, VideoSubsystem};
use std::collections::HashMap;
#[cfg(unix)]
use std::io::{BufRead, BufReader, ErrorKind, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};
use ui::{clock, BossStatus, Input, Ui};

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

fn sdl_rgb(c: Rgb) -> Color {
    Color::RGB(c.0, c.1, c.2)
}

/// Venue theme colours, sourced from `beercade_shared::theme`.
fn palette(theme: &str) -> Palette {
    let p = theme_palette(theme);
    Palette {
        bg: sdl_rgb(p.bg),
        panel: sdl_rgb(p.panel),
        accent: sdl_rgb(p.accent),
        accent2: sdl_rgb(p.accent2),
        warn: sdl_rgb(p.warn),
        text: sdl_rgb(p.text),
        dim: sdl_rgb(p.dim),
    }
}

/// First font that exists: venue font dropped on the image, DejaVu, or Windows fallbacks.
fn font_path(kind: &str) -> Option<&'static str> {
    let candidates: &[&'static str] = if kind == "display" {
        &[
            "/usr/share/fonts/beercade/display.ttf",
            "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            "C:\\Windows\\Fonts\\segoeui.ttf",
            "C:\\Windows\\Fonts\\arialbd.ttf",
            "C:\\Windows\\Fonts\\arial.ttf",
        ]
    } else {
        &[
            "/usr/share/fonts/beercade/data.ttf",
            "/usr/share/fonts/TTF/DejaVuSansMono-Bold.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf",
            "C:\\Windows\\Fonts\\consola.ttf",
            "C:\\Windows\\Fonts\\lucon.ttf",
            "C:\\Windows\\Fonts\\cour.ttf",
            "C:\\Windows\\Fonts\\arial.ttf",
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

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum FontSlot {
    Huge,
    Big,
    Mid,
    Small,
}

#[derive(PartialEq, Eq, Hash)]
struct TextKey {
    slot: FontSlot,
    text: String,
    color: (u8, u8, u8),
}

#[derive(PartialEq, Eq, Hash)]
struct QrKey {
    data: String,
    fg: (u8, u8, u8),
    bg: (u8, u8, u8),
    size: u32,
}

struct CachedTex<'tc> {
    tex: Texture<'tc>,
    w: u32,
    h: u32,
}

struct TextureCache<'tc> {
    text: HashMap<TextKey, CachedTex<'tc>>,
    qr: HashMap<QrKey, CachedTex<'tc>>,
}

impl<'tc> TextureCache<'tc> {
    fn new() -> Self {
        Self {
            text: HashMap::with_capacity(64),
            qr: HashMap::with_capacity(4),
        }
    }
}

struct Fonts<'ttf> {
    huge: Font<'ttf, 'static>,
    big: Font<'ttf, 'static>,
    mid: Font<'ttf, 'static>,
    small: Font<'ttf, 'static>,
}

impl<'ttf> Fonts<'ttf> {
    fn get(&self, slot: FontSlot) -> &Font<'ttf, 'static> {
        match slot {
            FontSlot::Huge => &self.huge,
            FontSlot::Big => &self.big,
            FontSlot::Mid => &self.mid,
            FontSlot::Small => &self.small,
        }
    }
}

/// Owned SDL2 display context. Dropping it destroys the window, renderer, and
/// `VideoSubsystem`, calling `SDL_VideoQuit()` to release KMSDRM master.
/// Field order ensures children drop before their parent SDL subsystems.
struct DisplayCtx {
    controllers: Vec<GameController>,
    events: EventPump,
    tc: TextureCreator<WindowContext>,
    canvas: Canvas<Window>,
    _video: VideoSubsystem,
    gc: GameControllerSubsystem,
    _sdl: Sdl,
}

impl DisplayCtx {
    fn open() -> Result<Self, String> {
        let sdl = sdl2::init()?;
        let video = sdl.video()?;
        let gc = sdl.game_controller()?;
        sdl2::hint::set("SDL_RENDER_SCALE_QUALITY", "1");

        let window = video
            .window("DOCKADE", W, H)
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
        let events = sdl.event_pump()?;

        Ok(Self {
            controllers: Vec::new(),
            events,
            tc,
            canvas,
            _video: video,
            gc,
            _sdl: sdl,
        })
    }
}

struct Painter<'a, 'ttf, 'tc> {
    canvas: &'a mut Canvas<Window>,
    tc: &'tc TextureCreator<WindowContext>,
    fonts: &'a Fonts<'ttf>,
    cache: &'a mut TextureCache<'tc>,
}

impl<'a, 'ttf, 'tc> Painter<'a, 'ttf, 'tc> {
    fn text(&mut self, slot: FontSlot, s: &str, color: Color, x: i32, y: i32, align: Align) {
        if s.is_empty() {
            return;
        }
        let key = TextKey {
            slot,
            text: s.to_string(),
            color: (color.r, color.g, color.b),
        };
        if !self.cache.text.contains_key(&key) {
            if self.cache.text.len() >= 256 {
                self.cache.text.clear();
            }
            let Ok(surface) = self.fonts.get(slot).render(s).blended(color) else {
                return;
            };
            let Ok(tex) = self.tc.create_texture_from_surface(&surface) else {
                return;
            };
            self.cache.text.insert(
                TextKey {
                    slot,
                    text: s.to_string(),
                    color: (color.r, color.g, color.b),
                },
                CachedTex {
                    tex,
                    w: surface.width(),
                    h: surface.height(),
                },
            );
        }
        if let Some(entry) = self.cache.text.get(&key) {
            let x = match align {
                Align::Left => x,
                Align::Center => x - entry.w as i32 / 2,
                Align::Right => x - entry.w as i32,
            };
            let _ = self.canvas.copy(&entry.tex, None, Rect::new(x, y, entry.w, entry.h));
        }
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
        let key = QrKey {
            data: data.to_string(),
            fg: (fg.r, fg.g, fg.b),
            bg: (bg.r, bg.g, bg.b),
            size,
        };
        if !self.cache.qr.contains_key(&key) {
            if self.cache.qr.len() >= 16 {
                self.cache.qr.clear();
            }
            let Ok(code) = qrcode::QrCode::new(data.as_bytes()) else {
                return;
            };
            let n = code.width() as u32;
            let quiet = 2;
            let cell = (size / (n + 2 * quiet)).max(1);
            let total = cell * (n + 2 * quiet);
            let Ok(mut surf) = Surface::new(total, total, PixelFormatEnum::RGB24) else {
                return;
            };
            let _ = surf.fill_rect(None, bg);
            for y in 0..n {
                for x in 0..n {
                    if code[(x as usize, y as usize)] == qrcode::Color::Dark {
                        let _ = surf.fill_rect(
                            Some(Rect::new(
                                ((x + quiet) * cell) as i32,
                                ((y + quiet) * cell) as i32,
                                cell,
                                cell,
                            )),
                            fg,
                        );
                    }
                }
            }
            let Ok(tex) = self.tc.create_texture_from_surface(&surf) else {
                return;
            };
            self.cache.qr.insert(
                QrKey {
                    data: data.to_string(),
                    fg: (fg.r, fg.g, fg.b),
                    bg: (bg.r, bg.g, bg.b),
                    size,
                },
                CachedTex {
                    tex,
                    w: total,
                    h: total,
                },
            );
        }
        if let Some(entry) = self.cache.qr.get(&key) {
            let (x0, y0) = (cx - entry.w as i32 / 2, cy - entry.h as i32 / 2);
            let _ = self.canvas.copy(&entry.tex, None, Rect::new(x0, y0, entry.w, entry.h));
        }
    }
}

/// Prominent retro "DOCKADE" logo banner with neon accents, drop-glow, and subtext.
fn draw_dockade_logo(p: &mut Painter, cx: i32, y: i32, c: &Palette) {
    p.fill(c.accent2, Rect::new(cx - 380, y - 18, 760, 2));
    p.fill(c.accent, Rect::new(cx - 300, y - 13, 600, 3));

    p.text(FontSlot::Huge, "DOCKADE", c.accent2, cx + 4, y + 4, Align::Center);
    p.text(FontSlot::Huge, "DOCKADE", c.accent, cx, y, Align::Center);

    p.fill(c.accent, Rect::new(cx - 300, y + 153, 600, 3));
    p.fill(c.accent2, Rect::new(cx - 380, y + 159, 760, 2));

    p.text(
        FontSlot::Small,
        "\u{25C6}   RETRO ARCADE & TAPROOM   \u{25C6}",
        c.dim,
        cx,
        y + 175,
        Align::Center,
    );
}

/// Standardized DOCKADE header bar used across session screens.
fn draw_header_bar(
    p: &mut Painter,
    node_id: &str,
    right_content: Option<(&str, Color)>,
    cx: i32,
    c: &Palette,
) {
    p.text(FontSlot::Small, &format!("STATION // {node_id}"), c.dim, 60, 25, Align::Left);

    p.fill(c.accent2, Rect::new(cx - 240, 48, 70, 2));
    p.text(FontSlot::Big, "DOCKADE", c.accent, cx, 20, Align::Center);
    p.fill(c.accent2, Rect::new(cx + 170, 48, 70, 2));

    if let Some((txt, col)) = right_content {
        p.text(FontSlot::Big, txt, col, W as i32 - 60, 20, Align::Right);
    }

    p.fill(c.panel, Rect::new(60, 85, W - 120, 2));
}

fn draw(p: &mut Painter, ui: &Ui, pulse: bool) {
    let Some(frame) = &ui.frame else {
        p.canvas.set_draw_color(Color::RGB(0, 0, 0));
        p.canvas.clear();
        return;
    };
    let c = palette(&frame.theme);
    p.canvas.set_draw_color(c.bg);
    p.canvas.clear();
    let cx = W as i32 / 2;

    match &frame.view {
        ShellView::Idle => {
            p.text(FontSlot::Small, &format!("STATION // {}", frame.node_id), c.accent, 60, 35, Align::Left);

            let status = ui.boss_status();
            let dot_col = match status {
                BossStatus::Online => Color::RGB(70, 230, 130),
                BossStatus::Syncing => c.warn,
                BossStatus::Standalone => c.dim,
            };
            let status_label = format!("\u{25CF} B.O.S.S. {}", status.as_str());
            p.text(FontSlot::Small, &status_label, dot_col, W as i32 - 60, 35, Align::Right);
            p.fill(c.panel, Rect::new(60, 75, W - 120, 2));

            draw_dockade_logo(p, cx, 175, &c);

            let cta_box = Rect::new(cx - 550, 420, 1100, 350);
            p.fill(c.panel, cta_box);
            let box_border = if pulse { c.accent } else { c.accent2 };
            p.frame_rect(box_border, cta_box, 2);

            p.text(FontSlot::Big, "READY PLAYER?", c.accent, cx, 455, Align::Center);
            let cta_color = if pulse { c.warn } else { c.accent2 };
            p.text(FontSlot::Big, "SEE THE BAR TO PLAY", cta_color, cx, 560, Align::Center);
            let kiosk_color = if pulse { c.text } else { c.dim };
            p.text(FontSlot::Mid, "Buy a game-time pass at the B.O.S.S. kiosk", kiosk_color, cx, 665, Align::Center);
            p.text(FontSlot::Small, "TAP RFID PASS OR SCAN PASS TO START", c.dim, cx, 725, Align::Center);

            p.text(
                FontSlot::Small,
                "DOCKADE ARCADE SYSTEM  \u{00B7}  POWERED BY B.E.E.R.C.A.D.E.",
                c.dim,
                cx,
                915,
                Align::Center,
            );

            p.fill(c.panel, Rect::new(60, 960, W - 120, 2));

            p.text(
                FontSlot::Small,
                "\u{25B2}\u{25BC} SELECT   \u{00B7}   A / START PLAY   \u{00B7}   Y / TAB CHANGE GAMES",
                c.dim,
                cx,
                995,
                Align::Center,
            );
        }
        ShellView::Staging { session_code, ready, total, time_remaining_sec } => {
            let time_str = format!("TIME {}", clock(*time_remaining_sec));
            draw_header_bar(p, &frame.node_id, Some((&time_str, c.warn)), cx, &c);

            p.text(FontSlot::Big, "GETTING YOUR GAMES READY", c.accent, cx, 340, Align::Center);
            let bar = Rect::new(cx - 600, 520, 1200, 40);
            p.frame_rect(c.accent, bar, 3);
            if *total > 0 {
                let w = (1194 * *ready / *total).max(1);
                p.fill(c.accent2, Rect::new(bar.x() + 3, bar.y() + 3, w, 34));
            }
            p.text(FontSlot::Mid, &format!("{ready} OF {total} GAMES"), c.text, cx, 600, Align::Center);
            p.text(FontSlot::Small, &format!("SESSION CODE {session_code}"), c.dim, cx, 960, Align::Center);
        }
        ShellView::Picker { session_code, time_remaining_sec, games } if ui.changing => {
            let _ = games;
            let time_str = format!("TIME {}", clock(*time_remaining_sec));
            draw_header_bar(p, &frame.node_id, Some((&time_str, c.warn)), cx, &c);

            p.text(FontSlot::Big, "CHANGE YOUR GAMES", c.accent, cx, 150, Align::Center);
            p.text(FontSlot::Mid, "Go to the B.O.S.S. kiosk, choose \"I'm already playing\"", c.text, cx, 270, Align::Center);
            p.text(FontSlot::Mid, "and enter this code. Your clock keeps running.", c.text, cx, 330, Align::Center);
            p.text(FontSlot::Huge, session_code, c.warn, cx - 300, 560, Align::Center);
            p.qr(session_code, Color::RGB(0, 0, 0), Color::RGB(255, 255, 255), cx + 420, 640, 360);

            p.fill(c.panel, Rect::new(60, 940, W - 120, 2));
            p.text(FontSlot::Small, "B / ESC  \u{00B7}  BACK TO YOUR GAMES", c.dim, cx, 980, Align::Center);
        }
        ShellView::Picker { session_code, time_remaining_sec, games } => {
            let low = *time_remaining_sec <= 300;
            let tc = if low && pulse { c.accent2 } else { c.warn };
            let time_str = format!("TIME {}", clock(*time_remaining_sec));
            draw_header_bar(p, &frame.node_id, Some((&time_str, tc)), cx, &c);

            p.text(FontSlot::Big, "PICK A GAME", c.accent, 60, 105, Align::Left);
            p.text(FontSlot::Small, &format!("{} AVAILABLE", games.len()), c.dim, W as i32 - 60, 120, Align::Right);

            if games.is_empty() {
                p.text(FontSlot::Mid, "No games ready yet. Please ask the staff.", c.text, cx, 480, Align::Center);
            }
            const ROWS: usize = 7;
            let first = ui.sel.saturating_sub(ROWS - 1);
            for (row, (i, g)) in games.iter().enumerate().skip(first).take(ROWS).enumerate() {
                let y = 175 + row as i32 * 96;
                let r = Rect::new(60, y, W - 120, 84);
                let selected = i == ui.sel;
                if selected {
                    p.fill(c.panel, r);
                    p.frame_rect(c.accent, r, 3);
                    p.text(FontSlot::Mid, "\u{25B6}", c.accent, 90, y + 18, Align::Left);
                }
                let col = if selected { c.accent } else { c.text };
                p.text(FontSlot::Mid, &g.title, col, 150, y + 18, Align::Left);
                p.text(FontSlot::Small, &g.system.to_uppercase(), c.dim, W as i32 - 100, y + 26, Align::Right);
            }
            if games.len() > ROWS {
                p.text(FontSlot::Small, &format!("{} / {}", ui.sel + 1, games.len()), c.dim, cx, 865, Align::Center);
            }

            p.fill(c.panel, Rect::new(60, 905, W - 120, 2));

            p.qr(session_code, Color::RGB(0, 0, 0), Color::RGB(255, 255, 255), 115, 985, 115);
            p.text(FontSlot::Mid, &format!("CODE: {session_code}"), c.warn, 195, 945, Align::Left);
            p.text(FontSlot::Small, "Scan at B.O.S.S. kiosk to top up or change games", c.dim, 195, 1000, Align::Left);

            p.text(
                FontSlot::Small,
                "\u{25B2}\u{25BC} SELECT   \u{00B7}   A PLAY   \u{00B7}   Y CHANGE GAMES",
                c.text,
                W as i32 - 60,
                955,
                Align::Right,
            );
            p.text(
                FontSlot::Small,
                "KEYBOARD: \u{2191}\u{2193} / W/S   \u{00B7}   ENTER / CTRL   \u{00B7}   TAB / Y",
                c.dim,
                W as i32 - 60,
                1000,
                Align::Right,
            );
        }
        ShellView::TimesUp { session_code, grace_left_sec } => {
            draw_header_bar(p, &frame.node_id, None, cx, &c);

            p.text(FontSlot::Huge, "TIME'S UP", c.accent, cx, 140, Align::Center);
            p.text(FontSlot::Huge, &grace_left_sec.to_string(), c.warn, cx - 420, 430, Align::Center);
            p.text(FontSlot::Mid, "SECONDS TO TOP UP", c.dim, cx - 420, 600, Align::Center);
            p.qr(session_code, Color::RGB(0, 0, 0), Color::RGB(255, 255, 255), cx + 380, 520, 320);
            p.text(FontSlot::Big, session_code, c.warn, cx + 380, 720, Align::Center);
            p.text(FontSlot::Mid, "Top up at the B.O.S.S. kiosk with this code", c.text, cx, 860, Align::Center);
            p.text(FontSlot::Small, "Your games are kept until the countdown ends", c.dim, cx, 940, Align::Center);
        }
        ShellView::Ended => {
            draw_header_bar(p, &frame.node_id, None, cx, &c);

            p.text(FontSlot::Huge, "THANKS FOR PLAYING", c.accent, cx, 380, Align::Center);
            p.text(FontSlot::Mid, "See you next round at DOCKADE", c.dim, cx, 600, Align::Center);
        }
        ShellView::Suspended => {
            p.canvas.set_draw_color(Color::RGB(0, 0, 0));
            p.canvas.clear();
        }
    }
}

/// Socket to the daemon, reconnecting as needed (Unix platforms).
#[cfg(unix)]
struct Link {
    stream: Option<(BufReader<UnixStream>, UnixStream)>,
    buf: String,
    retry_at: Instant,
}

#[cfg(unix)]
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

/// Stub socket link for non-Unix development and testing machines.
#[cfg(not(unix))]
struct Link {
    sent_mock: bool,
}

#[cfg(not(unix))]
impl Link {
    fn new() -> Self {
        Link { sent_mock: false }
    }

    fn poll(&mut self) -> Option<ShellFrame> {
        if !self.sent_mock {
            self.sent_mock = true;
            Some(ShellFrame {
                node_id: "DOCKADE-DEV-01".into(),
                theme: "vector".into(),
                view: ShellView::Idle,
            })
        } else {
            None
        }
    }

    fn send(&mut self, _a: &ShellAction) {}
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

#[derive(PartialEq, Eq)]
struct RenderKey {
    frame: Option<ShellFrame>,
    sel: usize,
    changing: bool,
    pulse: bool,
}

fn effective_pulse(ui: &Ui, now: Instant) -> bool {
    let raw = (now.elapsed().as_millis() / 600) % 2 == 0;
    match ui.frame.as_ref().map(|f| &f.view) {
        Some(ShellView::Idle) => raw,
        Some(ShellView::Picker { time_remaining_sec, .. })
            if !ui.changing && *time_remaining_sec <= 300 =>
        {
            raw
        }
        _ => false,
    }
}

fn is_suspended(ui: &Ui) -> bool {
    matches!(
        ui.frame.as_ref().map(|f| &f.view),
        Some(ShellView::Suspended)
    )
}

fn run_display_session(
    display: &mut DisplayCtx,
    fonts: &Fonts,
    link: &mut Link,
    ui: &mut Ui,
    started: Instant,
) {
    let mut cache = TextureCache::new();
    let mut last_axis = Instant::now();
    let mut last_rendered: Option<RenderKey> = None;

    loop {
        for ev in display.events.poll_iter() {
            let input = match ev {
                Event::Quit { .. } => None,
                Event::ControllerDeviceAdded { which, .. } => {
                    if let Ok(c) = display.gc.open(which) {
                        display.controllers.push(c);
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

        if is_suspended(ui) {
            return;
        }

        let pulse = effective_pulse(ui, started);
        let key = RenderKey {
            frame: ui.frame.clone(),
            sel: ui.sel,
            changing: ui.changing,
            pulse,
        };

        if last_rendered.as_ref() != Some(&key) {
            let mut painter = Painter {
                canvas: &mut display.canvas,
                tc: &display.tc,
                fonts,
                cache: &mut cache,
            };
            draw(&mut painter, ui, pulse);
            display.canvas.present();
            last_rendered = Some(key);
        }

        std::thread::sleep(Duration::from_millis(16));
    }
}

fn main() -> Result<(), String> {
    let ttf = sdl2::ttf::init().map_err(|e| e.to_string())?;
    let display_font = font_path("display").ok_or("no display font installed")?;
    let data_font = font_path("data").ok_or("no data font installed")?;
    let fonts = Fonts {
        huge: ttf.load_font(display_font, 150)?,
        big: ttf.load_font(display_font, 80)?,
        mid: ttf.load_font(data_font, 44)?,
        small: ttf.load_font(data_font, 30)?,
    };

    let mut link = Link::new();
    let mut ui = Ui::default();
    let started = Instant::now();

    loop {
        if is_suspended(&ui) {
            link.send(&ShellAction::DisplayReleased);
            while is_suspended(&ui) {
                if let Some(f) = link.poll() {
                    ui.set_frame(f);
                    if !is_suspended(&ui) {
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        let mut display = DisplayCtx::open()?;
        run_display_session(&mut display, &fonts, &mut link, &mut ui, started);
        // `display` drops here, calling SDL_VideoQuit() and releasing KMSDRM master.
        drop(display);
    }
}
