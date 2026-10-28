//! A.S.T.R.O. in-game overlay.
//!
//! RetroArch owns the KMS display while a game runs, so nothing else can draw
//! on top of it. The overlay therefore speaks RetroArch's own network command
//! protocol (UDP, localhost only) and renders through its OSD:
//!
//! * HUD: session time, credits and netplay link, shown at milestones rather
//!   than every second so the game stays readable.
//! * Toasts: credits added, session started, score posted, order updates.
//! * Time's up: the game is paused and a CONTINUE? countdown runs; more time or
//!   credits inside the window resumes play, otherwise the game is closed.
//!
//! The OSD colour follows the venue theme picked on B.O.S.S.
//! (`GET /api/v1/ui/theme`), written to an appendconfig the launcher passes to
//! RetroArch at start-up.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::net::UdpSocket;

/// RetroArch's default network command port.
pub const RA_CMD_PORT: u16 = 55355;
/// Length of the CONTINUE? window after a session runs out.
pub const CONTINUE_SECS: u32 = 10;
pub const DEFAULT_THEME: &str = "taproom";

/// OSD colour per venue theme: each theme's accent, except Big Sky, whose
/// navy accent is unreadable over a game, so it uses its sky blue.
pub fn theme_rgb(theme: &str) -> (u8, u8, u8) {
    match theme {
        "vector" => (0x5c, 0xf2, 0xff),
        "bigsky" => (0x4f, 0xa3, 0xff),
        "pinball" => (0xff, 0xc9, 0x4a),
        "classic" => (0x00, 0xf0, 0xff),
        _ => (0xf2, 0xa9, 0x00),
    }
}

/// RetroArch appendconfig enabling the command port and theming the OSD.
pub fn retroarch_cfg(theme: &str) -> String {
    let (r, g, b) = theme_rgb(theme);
    let f = |c: u8| format!("{:.3}", c as f32 / 255.0);
    format!(
        "network_cmd_enable = \"true\"\n\
         network_cmd_port = \"{RA_CMD_PORT}\"\n\
         video_font_enable = \"true\"\n\
         video_font_size = \"36.000000\"\n\
         video_msg_pos_x = \"0.040000\"\n\
         video_msg_pos_y = \"0.060000\"\n\
         video_msg_color_r = \"{}\"\n\
         video_msg_color_g = \"{}\"\n\
         video_msg_color_b = \"{}\"\n\
         video_msg_bgcolor_enable = \"true\"\n\
         video_msg_bgcolor_red = \"4\"\n\
         video_msg_bgcolor_green = \"6\"\n\
         video_msg_bgcolor_blue = \"10\"\n\
         video_msg_bgcolor_opacity = \"0.780000\"\n",
        f(r),
        f(g),
        f(b)
    )
}

/// Where the launcher picks the overlay config up from.
pub fn cfg_path(data_dir: &Path) -> PathBuf {
    data_dir.join("cache").join("astro-overlay.cfg")
}

/// Write the themed config atomically. A game already running keeps its
/// colours until the next launch.
pub fn write_cfg(data_dir: &Path, theme: &str) -> std::io::Result<()> {
    let path = cfg_path(data_dir);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("cfg.tmp");
    std::fs::write(&tmp, retroarch_cfg(theme))?;
    std::fs::rename(tmp, path)
}

/// Ask B.O.S.S. for the venue theme. `None` when unreachable or unknown.
pub async fn fetch_theme(client: &reqwest::Client, boss_url: &str) -> Option<String> {
    let url = format!("{}/api/v1/ui/theme", boss_url.trim_end_matches('/'));
    let body: serde_json::Value = client.get(url).send().await.ok()?.json().await.ok()?;
    let theme = body.get("theme")?.as_str()?.to_string();
    let known = ["taproom", "vector", "bigsky", "pinball", "classic"];
    known.contains(&theme.as_str()).then_some(theme)
}

pub fn fmt_clock(secs: u32) -> String {
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

/// One HUD line, e.g. `TIME 38:00 · CREDIT 03 · LINK ▸ ASTRO-02`.
pub fn hud_line(time_remaining: Option<u32>, credits: u32, link_peer: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(t) = time_remaining {
        parts.push(format!("TIME {}", fmt_clock(t)));
    }
    parts.push(format!("CREDIT {credits:02}"));
    if let Some(peer) = link_peer {
        parts.push(format!("LINK \u{25B8} {peer}"));
    }
    parts.join(" \u{00B7} ")
}

/// Whether the HUD is worth showing at this many seconds left: on the five
/// minute marks, every 10 s in the last minute and every second in the last
/// ten.
pub fn hud_due(time_remaining: u32) -> bool {
    match time_remaining {
        0 => false,
        1..=10 => true,
        11..=60 => time_remaining % 10 == 0,
        _ => time_remaining % 300 == 0,
    }
}

pub fn continue_line(node_id: &str, secs_left: u32) -> String {
    format!("CONTINUE? {secs_left} \u{00B7} TIME'S UP ON {node_id} \u{00B7} TAP WRISTBAND OR PICK MORE TIME")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    Resume,
    AddCredits,
    Order,
    Scores,
    Netplay,
    Staff,
    End,
}

pub const MENU_ITEMS: [MenuItem; 7] = [
    MenuItem::Resume,
    MenuItem::AddCredits,
    MenuItem::Order,
    MenuItem::Scores,
    MenuItem::Netplay,
    MenuItem::Staff,
    MenuItem::End,
];

impl MenuItem {
    pub fn label(self) -> &'static str {
        match self {
            MenuItem::Resume => "RESUME",
            MenuItem::AddCredits => "ADD CREDITS",
            MenuItem::Order => "ORDER A DRINK",
            MenuItem::Scores => "HIGH SCORES",
            MenuItem::Netplay => "NETPLAY",
            MenuItem::Staff => "CALL STAFF",
            MenuItem::End => "END SESSION",
        }
    }
}

/// What the daemon has to do after a menu input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuEffect {
    None,
    /// Pause the game and show the menu.
    Open,
    /// Hide the menu and resume the game.
    Close,
    /// The menu changed: redraw it.
    Redraw,
    CallStaff,
    EndSession,
}

/// Facts the menu text needs from the daemon.
pub struct MenuContext<'a> {
    pub node_id: &'a str,
    pub credits: u32,
    pub time_remaining: Option<u32>,
    pub link_peer: Option<&'a str>,
}

/// The pause menu from the design, driven by cabinet controls and drawn as
/// one OSD line (RetroArch shows a single message at a time).
#[derive(Debug, Default)]
pub struct PauseMenu {
    pub open: bool,
    sel: usize,
    /// Showing the selected item's detail panel.
    detail: bool,
    /// END SESSION pressed once; a second OK confirms.
    confirm_end: bool,
}

impl PauseMenu {
    pub fn selected(&self) -> MenuItem {
        MENU_ITEMS[self.sel]
    }

    pub fn handle(&mut self, input: crate::input::MenuInput) -> MenuEffect {
        use crate::input::MenuInput::*;
        if !self.open {
            if input == Toggle {
                *self = PauseMenu { open: true, ..Default::default() };
                return MenuEffect::Open;
            }
            return MenuEffect::None;
        }
        match input {
            Toggle => self.close(),
            Back if self.detail || self.confirm_end => {
                self.detail = false;
                self.confirm_end = false;
                MenuEffect::Redraw
            }
            Back => self.close(),
            Up | Down if self.detail || self.confirm_end => MenuEffect::None,
            Up => {
                self.sel = (self.sel + MENU_ITEMS.len() - 1) % MENU_ITEMS.len();
                MenuEffect::Redraw
            }
            Down => {
                self.sel = (self.sel + 1) % MENU_ITEMS.len();
                MenuEffect::Redraw
            }
            Ok => match self.selected() {
                MenuItem::Resume => self.close(),
                MenuItem::End if self.confirm_end => {
                    self.open = false;
                    MenuEffect::EndSession
                }
                MenuItem::End => {
                    self.confirm_end = true;
                    MenuEffect::Redraw
                }
                MenuItem::Staff if !self.detail => {
                    self.detail = true;
                    MenuEffect::CallStaff
                }
                _ => {
                    self.detail = true;
                    MenuEffect::Redraw
                }
            },
        }
    }

    fn close(&mut self) -> MenuEffect {
        self.open = false;
        MenuEffect::Close
    }

    pub fn render(&self, ctx: &MenuContext) -> String {
        let item = self.selected();
        if self.confirm_end {
            return format!(
                "END YOUR SESSION? \u{00B7} RETURNS {} TO THE GAME MENU \u{00B7} A CONFIRM \u{00B7} B BACK",
                ctx.node_id
            );
        }
        if self.detail {
            let body = match item {
                MenuItem::AddCredits => format!(
                    "CREDIT {:02} \u{00B7} TAP YOUR WRISTBAND ON THE READER, OR SCAN THE QR AT THE KIOSK",
                    ctx.credits
                ),
                MenuItem::Order => format!(
                    "ORDER AT THE KIOSK OR THE BAR \u{00B7} WE BRING IT TO {}",
                    ctx.node_id
                ),
                MenuItem::Scores => "YOUR SCORES POST TO THE VENUE TVS AUTOMATICALLY".to_string(),
                MenuItem::Netplay => match ctx.link_peer {
                    Some(peer) => format!("LINKED WITH {peer} \u{00B7} ROLLBACK KEEPS INPUTS TIGHT"),
                    None => "NOT LINKED \u{00B7} SOLO PLAY".to_string(),
                },
                MenuItem::Staff => format!("STAFF CALLED \u{00B7} SOMEONE IS ON THE WAY TO {}", ctx.node_id),
                MenuItem::Resume | MenuItem::End => String::new(),
            };
            return format!("{} \u{00B7} {body} \u{00B7} B BACK", item.label());
        }
        let time = ctx
            .time_remaining
            .map(|t| format!(" \u{00B7} TIME {}", fmt_clock(t)))
            .unwrap_or_default();
        format!(
            "{} \u{00B7} PAUSED{time} \u{00B7} \u{25B6} {} ({}/{}) \u{00B7} \u{25B2}\u{25BC} SELECT \u{00B7} A OK \u{00B7} B BACK",
            ctx.node_id,
            item.label(),
            self.sel + 1,
            MENU_ITEMS.len()
        )
    }
}

/// Fire-and-forget sender for RetroArch network commands. Every call is a
/// no-op when no game is running (nothing listens on the port).
#[derive(Clone, Debug)]
pub struct RetroArch {
    port: u16,
}

impl Default for RetroArch {
    fn default() -> Self {
        Self { port: RA_CMD_PORT }
    }
}

impl RetroArch {
    async fn send(&self, cmd: &str) -> std::io::Result<()> {
        let sock = UdpSocket::bind("127.0.0.1:0").await?;
        sock.send_to(cmd.as_bytes(), ("127.0.0.1", self.port)).await?;
        Ok(())
    }

    pub async fn show(&self, msg: &str) {
        // The command is line based: keep the message on one line.
        let msg = msg.replace(['\n', '\r'], " ");
        let _ = self.send(&format!("SHOW_MSG {msg}")).await;
    }

    /// `Some(true)` when paused, `None` when RetroArch did not answer.
    pub async fn is_paused(&self) -> Option<bool> {
        let sock = UdpSocket::bind("127.0.0.1:0").await.ok()?;
        sock.send_to(b"GET_STATUS", ("127.0.0.1", self.port)).await.ok()?;
        let mut buf = [0u8; 512];
        let n = tokio::time::timeout(Duration::from_millis(250), sock.recv(&mut buf))
            .await
            .ok()?
            .ok()?;
        let reply = String::from_utf8_lossy(&buf[..n]);
        if reply.contains("PAUSED") {
            Some(true)
        } else if reply.contains("PLAYING") {
            Some(false)
        } else {
            None
        }
    }

    /// RetroArch only has a toggle, so check the state first.
    pub async fn set_paused(&self, paused: bool) {
        if self.is_paused().await == Some(!paused) {
            let _ = self.send("PAUSE_TOGGLE").await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hud_line_matches_design() {
        assert_eq!(
            hud_line(Some(38 * 60), 3, Some("ASTRO-02")),
            "TIME 38:00 \u{00B7} CREDIT 03 \u{00B7} LINK \u{25B8} ASTRO-02"
        );
        assert_eq!(hud_line(None, 12, None), "CREDIT 12");
    }

    #[test]
    fn hud_milestones() {
        assert!(hud_due(1800));
        assert!(!hud_due(1799));
        assert!(hud_due(50));
        assert!(!hud_due(55));
        assert!(hud_due(7));
        assert!(!hud_due(0));
    }

    #[test]
    fn cfg_carries_theme_colour_and_port() {
        let cfg = retroarch_cfg("vector");
        assert!(cfg.contains("network_cmd_enable = \"true\""));
        assert!(cfg.contains("network_cmd_port = \"55355\""));
        assert!(cfg.contains("video_msg_color_r = \"0.361\""));
        // Unknown themes fall back to the taproom amber.
        assert!(retroarch_cfg("nope").contains("video_msg_color_r = \"0.949\""));
    }

    #[test]
    fn cfg_written_atomically() {
        let dir = std::env::temp_dir().join(format!("astro-overlay-{}", std::process::id()));
        write_cfg(&dir, "pinball").unwrap();
        let text = std::fs::read_to_string(cfg_path(&dir)).unwrap();
        assert!(text.contains("video_msg_color_g = \"0.788\""));
        assert!(!cfg_path(&dir).with_extension("cfg.tmp").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    fn ctx() -> MenuContext<'static> {
        MenuContext { node_id: "ASTRO-01", credits: 3, time_remaining: Some(125), link_peer: None }
    }

    #[test]
    fn menu_opens_navigates_and_resumes() {
        use crate::input::MenuInput::*;
        let mut m = PauseMenu::default();
        assert_eq!(m.handle(Down), MenuEffect::None, "closed menu ignores nav");
        assert_eq!(m.handle(Toggle), MenuEffect::Open);
        assert!(m.render(&ctx()).contains("PAUSED \u{00B7} TIME 02:05 \u{00B7} \u{25B6} RESUME (1/7)"));
        assert_eq!(m.handle(Up), MenuEffect::Redraw);
        assert_eq!(m.selected(), MenuItem::End, "wraps to the bottom");
        m.handle(Down);
        assert_eq!(m.handle(Ok), MenuEffect::Close);
        assert!(!m.open);
    }

    #[test]
    fn detail_and_back() {
        use crate::input::MenuInput::*;
        let mut m = PauseMenu::default();
        m.handle(Toggle);
        m.handle(Down);
        assert_eq!(m.handle(Ok), MenuEffect::Redraw);
        assert!(m.render(&ctx()).starts_with("ADD CREDITS \u{00B7} CREDIT 03"));
        assert_eq!(m.handle(Down), MenuEffect::None, "nav locked in detail");
        assert_eq!(m.handle(Back), MenuEffect::Redraw);
        assert_eq!(m.handle(Back), MenuEffect::Close);
    }

    #[test]
    fn end_session_needs_confirm_and_staff_calls_once() {
        use crate::input::MenuInput::*;
        let mut m = PauseMenu::default();
        m.handle(Toggle);
        m.handle(Up);
        m.handle(Up);
        assert_eq!(m.handle(Ok), MenuEffect::CallStaff);
        assert_eq!(m.handle(Ok), MenuEffect::Redraw, "no second call while shown");
        m.handle(Back);
        m.handle(Down);
        assert_eq!(m.handle(Ok), MenuEffect::Redraw);
        assert!(m.render(&ctx()).starts_with("END YOUR SESSION?"));
        assert_eq!(m.handle(Ok), MenuEffect::EndSession);
        assert!(!m.open);
    }

    #[tokio::test]
    async fn show_msg_reaches_port() {
        let rx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let ra = RetroArch { port: rx.local_addr().unwrap().port() };
        ra.show("CREDIT 03\nnext").await;
        let mut buf = [0u8; 128];
        let n = rx.recv(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"SHOW_MSG CREDIT 03 next");
    }
}
