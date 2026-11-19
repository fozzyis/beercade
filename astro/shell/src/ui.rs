//! Screen state and input handling, independent of SDL so it can be tested.

use beercade_shared::shell::{ShellAction, ShellFrame, ShellView};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Up,
    Down,
    Ok,
    Back,
    /// "Change my games" (Y / Tab / Select).
    Change,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BossStatus {
    Online,
    Syncing,
    Standalone,
}

impl BossStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            BossStatus::Online => "ONLINE",
            BossStatus::Syncing => "SYNCING",
            BossStatus::Standalone => "STANDALONE",
        }
    }
}

/// Local screen state on top of the daemon's frame.
#[derive(Debug, Default)]
pub struct Ui {
    pub frame: Option<ShellFrame>,
    pub sel: usize,
    /// Showing the "change my games" code screen over the picker.
    pub changing: bool,
    pub boss_status: Option<BossStatus>,
}

impl Ui {
    pub fn set_frame(&mut self, f: ShellFrame) {
        if let ShellView::Picker { games, .. } = &f.view {
            self.sel = self.sel.min(games.len().saturating_sub(1));
        } else {
            self.changing = false;
        }
        self.frame = Some(f);
    }

    /// Derives the current B.O.S.S. connection status.
    pub fn boss_status(&self) -> BossStatus {
        if let Some(s) = self.boss_status {
            return s;
        }
        if let Some(f) = &self.frame {
            let id = f.node_id.to_ascii_uppercase();
            if id.contains("STANDALONE") || id.contains("OFFLINE") || id.contains("LOCAL") {
                BossStatus::Standalone
            } else if matches!(f.view, ShellView::Staging { .. }) {
                BossStatus::Syncing
            } else {
                BossStatus::Online
            }
        } else {
            BossStatus::Standalone
        }
    }

    /// Returns what to send to the daemon, if anything.
    pub fn input(&mut self, i: Input) -> Option<ShellAction> {
        let ShellView::Picker { games, .. } = &self.frame.as_ref()?.view else {
            return None;
        };
        if self.changing {
            if matches!(i, Input::Back | Input::Change | Input::Ok) {
                self.changing = false;
            }
            return None;
        }
        let n = games.len();
        match i {
            Input::Up if n > 0 => self.sel = (self.sel + n - 1) % n,
            Input::Down if n > 0 => self.sel = (self.sel + 1) % n,
            Input::Ok => return games.get(self.sel).map(|g| ShellAction::Launch { game_id: g.game_id.clone() }),
            Input::Change => {
                self.changing = true;
                return Some(ShellAction::ChangeGames);
            }
            _ => {}
        }
        None
    }
}

/// mm:ss, or h:mm:ss past an hour.
pub fn clock(secs: u32) -> String {
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
    } else {
        format!("{:02}:{:02}", secs / 60, secs % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beercade_shared::shell::ShellGame;

    fn picker(n: usize) -> ShellFrame {
        ShellFrame {
            node_id: "ASTRO-01".into(),
            theme: "vector".into(),
            view: ShellView::Picker {
                session_code: "BRC-1".into(),
                time_remaining_sec: 600,
                games: (0..n)
                    .map(|i| ShellGame { game_id: format!("g{i}"), title: format!("G{i}"), system: "snes".into() })
                    .collect(),
            },
        }
    }

    #[test]
    fn picks_wrap_and_launch() {
        let mut ui = Ui::default();
        ui.set_frame(picker(3));
        assert_eq!(ui.input(Input::Up), None);
        assert_eq!(ui.sel, 2);
        assert_eq!(ui.input(Input::Ok), Some(ShellAction::Launch { game_id: "g2".into() }));
    }

    #[test]
    fn change_screen_blocks_launch_until_back() {
        let mut ui = Ui::default();
        ui.set_frame(picker(2));
        assert_eq!(ui.input(Input::Change), Some(ShellAction::ChangeGames));
        assert_eq!(ui.input(Input::Ok), None);
        assert!(!ui.changing);
        assert!(ui.input(Input::Ok).is_some());
    }

    #[test]
    fn shrinking_list_keeps_selection_valid() {
        let mut ui = Ui::default();
        ui.set_frame(picker(5));
        ui.sel = 4;
        ui.set_frame(picker(2));
        assert_eq!(ui.sel, 1);
    }

    #[test]
    fn no_input_outside_picker() {
        let mut ui = Ui::default();
        ui.set_frame(ShellFrame { node_id: "A".into(), theme: "t".into(), view: ShellView::Idle });
        assert_eq!(ui.input(Input::Ok), None);
        assert_eq!(clock(3725), "1:02:05");
        assert_eq!(clock(125), "02:05");
    }

    #[test]
    fn boss_status_derivation() {
        let mut ui = Ui::default();
        assert_eq!(ui.boss_status(), BossStatus::Standalone);

        ui.set_frame(ShellFrame { node_id: "DOCKADE-01".into(), theme: "vector".into(), view: ShellView::Idle });
        assert_eq!(ui.boss_status(), BossStatus::Online);

        ui.set_frame(ShellFrame {
            node_id: "DOCKADE-01".into(),
            theme: "vector".into(),
            view: ShellView::Staging {
                session_code: "B1".into(),
                ready: 1,
                total: 2,
                time_remaining_sec: 100,
            },
        });
        assert_eq!(ui.boss_status(), BossStatus::Syncing);

        ui.set_frame(ShellFrame {
            node_id: "CABINET-STANDALONE".into(),
            theme: "vector".into(),
            view: ShellView::Idle,
        });
        assert_eq!(ui.boss_status(), BossStatus::Standalone);
    }
}
