//! A.S.T.R.O. session state machine.
//!
//! Pure logic: the daemon feeds it commands, game start/exit and a 1 Hz tick,
//! and carries out the [`Effect`]s it returns. Keeping I/O out makes the
//! timing rules (expiry, CONTINUE?, top-up grace, file wipe) testable.
//!
//! ```text
//! Staging ─staged─► Picker ◄─exited── InGame
//!    │                 │ started ─────►  │
//!    └─────── time 0 ──┴────────┬────────┘
//!                               ▼
//!          Expired (game paused CONTINUE? → closed; files kept for grace)
//!            │ add_time → Picker / InGame        │ grace 0 or end()
//!            ▼                                   ▼
//!                                              Ended (files wiped)
//! ```

use beercade_shared::models::{CabinetEvent, GameRef};

/// Grace window used when B.O.S.S. doesn't send one (older builds).
pub const DEFAULT_GRACE_SECS: u32 = 90;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// Fetching the session's games.
    Staging,
    /// The customer shell shows the game list.
    Picker,
    InGame { game_id: String },
    /// Time ran out. The game (if any) is paused for `continue_left` seconds,
    /// then closed; files stay until `grace_left` reaches zero.
    Expired {
        grace_left: u32,
        continue_left: u32,
        paused_game: Option<String>,
    },
    Ended,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Event(CabinetEvent),
    PauseGame,
    ResumeGame,
    CloseGame,
    /// Stage these games (new or replacement list).
    Stage(Vec<GameRef>),
    /// Delete staged files not in the current list.
    Prune,
    /// Delete every session file.
    Wipe,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub code: String,
    pub remaining: u32,
    pub grace_secs: u32,
    pub continue_secs: u32,
    pub games: Vec<GameRef>,
    pub ready: Vec<String>,
    pub phase: Phase,
}

impl Session {
    /// A new session; the caller stages the games returned in the effects.
    pub fn start(
        id: &str,
        code: &str,
        duration_secs: u32,
        grace_secs: u32,
        continue_secs: u32,
        games: Vec<GameRef>,
    ) -> (Self, Vec<Effect>) {
        let grace_secs = if grace_secs == 0 { DEFAULT_GRACE_SECS } else { grace_secs };
        let s = Session {
            id: id.to_string(),
            code: code.to_string(),
            remaining: duration_secs,
            grace_secs,
            continue_secs: continue_secs.min(grace_secs),
            games: games.clone(),
            ready: Vec::new(),
            phase: if games.is_empty() { Phase::Picker } else { Phase::Staging },
        };
        let fx = if games.is_empty() { Vec::new() } else { vec![Effect::Stage(games)] };
        (s, fx)
    }

    pub fn is_live(&self) -> bool {
        !matches!(self.phase, Phase::Ended)
    }

    pub fn game(&self, game_id: &str) -> Option<&GameRef> {
        self.games.iter().find(|g| g.game_id == game_id)
    }

    /// Games ready to play, in the order B.O.S.S. sent them.
    pub fn playable(&self) -> Vec<&GameRef> {
        self.games.iter().filter(|g| self.ready.contains(&g.game_id)).collect()
    }

    /// Staging finished.
    pub fn staged(&mut self, ready: Vec<String>, failed: Vec<String>) -> Vec<Effect> {
        for id in ready.iter() {
            if !self.ready.contains(id) && self.game(id).is_some() {
                self.ready.push(id.clone());
            }
        }
        if self.phase == Phase::Staging {
            self.phase = Phase::Picker;
        }
        vec![Effect::Event(CabinetEvent::SessionStaged {
            session_id: self.id.clone(),
            ready_game_ids: ready,
            failed_game_ids: failed,
        })]
    }

    pub fn game_started(&mut self, game_id: &str) -> Vec<Effect> {
        if !matches!(self.phase, Phase::Picker | Phase::Staging | Phase::InGame { .. }) {
            return Vec::new();
        }
        self.phase = Phase::InGame { game_id: game_id.to_string() };
        vec![Effect::Event(CabinetEvent::GameStarted {
            session_id: self.id.clone(),
            game_id: game_id.to_string(),
        })]
    }

    pub fn game_exited(&mut self, game_id: &str, crashed: bool) -> Vec<Effect> {
        match &mut self.phase {
            Phase::InGame { .. } => self.phase = Phase::Picker,
            Phase::Expired { paused_game, .. } => *paused_game = None,
            _ => {}
        }
        vec![Effect::Event(CabinetEvent::GameExited {
            session_id: self.id.clone(),
            game_id: game_id.to_string(),
            crashed,
        })]
    }

    /// One second passed.
    pub fn tick(&mut self) -> Vec<Effect> {
        match &mut self.phase {
            Phase::Staging | Phase::Picker | Phase::InGame { .. } => {
                self.remaining = self.remaining.saturating_sub(1);
                if self.remaining > 0 {
                    return Vec::new();
                }
                let paused_game = match &self.phase {
                    Phase::InGame { game_id } => Some(game_id.clone()),
                    _ => None,
                };
                let mut fx = Vec::new();
                if paused_game.is_some() {
                    fx.push(Effect::PauseGame);
                }
                self.phase = Phase::Expired {
                    grace_left: self.grace_secs,
                    continue_left: self.continue_secs,
                    paused_game,
                };
                fx.push(Effect::Event(CabinetEvent::SessionExpired { session_id: self.id.clone() }));
                fx
            }
            Phase::Expired { grace_left, continue_left, paused_game } => {
                let mut fx = Vec::new();
                *grace_left = grace_left.saturating_sub(1);
                if paused_game.is_some() {
                    *continue_left = continue_left.saturating_sub(1);
                    if *continue_left == 0 {
                        *paused_game = None;
                        fx.push(Effect::CloseGame);
                    }
                }
                let grace_over = *grace_left == 0;
                if grace_over {
                    fx.extend(self.finish());
                }
                fx
            }
            Phase::Ended => Vec::new(),
        }
    }

    /// More time bought (top-up or upsell).
    pub fn add_time(&mut self, secs: u32) -> Vec<Effect> {
        if secs == 0 || !self.is_live() {
            return Vec::new();
        }
        self.remaining = self.remaining.saturating_add(secs);
        if let Phase::Expired { paused_game, .. } = &self.phase {
            return match paused_game.clone() {
                Some(game_id) => {
                    self.phase = Phase::InGame { game_id };
                    vec![Effect::ResumeGame]
                }
                None => {
                    self.phase = Phase::Picker;
                    Vec::new()
                }
            };
        }
        Vec::new()
    }

    /// New game list from B.O.S.S. The running game keeps running even if it
    /// was dropped; its file is pruned once it exits.
    pub fn replace_games(&mut self, games: Vec<GameRef>) -> Vec<Effect> {
        if !self.is_live() {
            return Vec::new();
        }
        self.ready.retain(|id| games.iter().any(|g| &g.game_id == id));
        let new: Vec<GameRef> =
            games.iter().filter(|g| !self.ready.contains(&g.game_id)).cloned().collect();
        self.games = games;
        let mut fx = vec![Effect::Prune];
        if !new.is_empty() {
            fx.push(Effect::Stage(new));
        }
        fx
    }

    /// The player chose "Change my games".
    pub fn change_requested(&self) -> Vec<Effect> {
        vec![Effect::Event(CabinetEvent::ChangeRequested { session_id: self.id.clone() })]
    }

    /// End now (B.O.S.S. EndSession or END SESSION on the pause menu).
    pub fn end(&mut self) -> Vec<Effect> {
        if !self.is_live() {
            return Vec::new();
        }
        let running = matches!(
            self.phase,
            Phase::InGame { .. } | Phase::Expired { paused_game: Some(_), .. }
        );
        let mut fx = if running { vec![Effect::CloseGame] } else { Vec::new() };
        fx.extend(self.finish());
        fx
    }

    fn finish(&mut self) -> Vec<Effect> {
        self.phase = Phase::Ended;
        self.ready.clear();
        vec![
            Effect::Wipe,
            Effect::Event(CabinetEvent::SessionEnded {
                session_id: self.id.clone(),
                files_wiped: true,
            }),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(id: &str) -> GameRef {
        GameRef {
            game_id: id.into(),
            title: id.into(),
            system: "snes".into(),
            core: "snes9x".into(),
            file_name: format!("{id}.zip"),
            size_bytes: 1,
            sha256: String::new(),
            artwork_url: None,
        }
    }

    fn running(dur: u32, grace: u32) -> Session {
        let (mut s, fx) = Session::start("s1", "BRC-1", dur, grace, 10, vec![g("a"), g("b")]);
        assert_eq!(fx, vec![Effect::Stage(vec![g("a"), g("b")])]);
        s.staged(vec!["a".into(), "b".into()], vec![]);
        s
    }

    fn ticks(s: &mut Session, n: u32) -> Vec<Effect> {
        (0..n).flat_map(|_| s.tick()).collect()
    }

    fn has_event(fx: &[Effect], f: impl Fn(&CabinetEvent) -> bool) -> bool {
        fx.iter().any(|e| matches!(e, Effect::Event(ev) if f(ev)))
    }

    #[test]
    fn staging_reports_ready_and_failed() {
        let (mut s, _) = Session::start("s1", "C", 60, 30, 10, vec![g("a"), g("b")]);
        let fx = s.staged(vec!["a".into()], vec!["b".into()]);
        assert_eq!(s.phase, Phase::Picker);
        assert_eq!(s.playable(), vec![&g("a")]);
        assert!(has_event(&fx, |e| matches!(e, CabinetEvent::SessionStaged { failed_game_ids, .. } if failed_game_ids == &vec!["b".to_string()])));
    }

    #[test]
    fn expiry_in_game_pauses_then_closes_then_wipes() {
        let mut s = running(5, 30);
        s.game_started("a");
        let fx = ticks(&mut s, 5);
        assert!(fx.contains(&Effect::PauseGame));
        assert!(has_event(&fx, |e| matches!(e, CabinetEvent::SessionExpired { .. })));
        let fx = ticks(&mut s, 10);
        assert_eq!(fx, vec![Effect::CloseGame]);
        let fx = ticks(&mut s, 20);
        assert_eq!(s.phase, Phase::Ended);
        assert!(fx.contains(&Effect::Wipe));
        assert!(has_event(&fx, |e| matches!(e, CabinetEvent::SessionEnded { files_wiped: true, .. })));
    }

    #[test]
    fn top_up_during_continue_resumes_the_game() {
        let mut s = running(3, 30);
        s.game_started("a");
        ticks(&mut s, 4);
        assert_eq!(s.add_time(900), vec![Effect::ResumeGame]);
        assert_eq!(s.phase, Phase::InGame { game_id: "a".into() });
        assert_eq!(s.remaining, 900);
    }

    #[test]
    fn top_up_after_game_closed_returns_to_picker_with_files() {
        let mut s = running(2, 30);
        s.game_started("a");
        ticks(&mut s, 2 + 10);
        s.game_exited("a", false);
        assert!(s.add_time(600).is_empty());
        assert_eq!(s.phase, Phase::Picker);
        assert_eq!(s.playable().len(), 2);
    }

    #[test]
    fn expiry_in_picker_has_no_game_to_pause() {
        let mut s = running(1, 5);
        let fx = s.tick();
        assert!(!fx.contains(&Effect::PauseGame));
        let fx = ticks(&mut s, 5);
        assert!(!fx.contains(&Effect::CloseGame));
        assert!(fx.contains(&Effect::Wipe));
    }

    #[test]
    fn missing_grace_uses_default() {
        let (s, _) = Session::start("s", "C", 60, 0, 10, vec![]);
        assert_eq!(s.grace_secs, DEFAULT_GRACE_SECS);
        assert_eq!(s.phase, Phase::Picker);
    }

    #[test]
    fn replacing_games_stages_only_new_ones() {
        let mut s = running(600, 30);
        let fx = s.replace_games(vec![g("b"), g("c")]);
        assert_eq!(fx, vec![Effect::Prune, Effect::Stage(vec![g("c")])]);
        assert_eq!(s.ready, vec!["b".to_string()]);
    }

    #[test]
    fn end_closes_running_game_and_wipes() {
        let mut s = running(600, 30);
        s.game_started("a");
        let fx = s.end();
        assert_eq!(fx[0], Effect::CloseGame);
        assert!(fx.contains(&Effect::Wipe));
        assert!(s.end().is_empty(), "ending twice does nothing");
        assert!(s.add_time(60).is_empty());
    }
}
