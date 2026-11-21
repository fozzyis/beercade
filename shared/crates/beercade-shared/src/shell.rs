//! Local IPC between `astro-daemon` and the `astro-shell` customer screen.
//!
//! The daemon owns all state; the shell only draws what it is told and sends
//! back what the player picked. JSON lines over a Unix socket: the daemon
//! writes a [`ShellFrame`] whenever the view changes (and at least every
//! second for the clock), the shell writes [`ShellAction`]s.

use serde::{Deserialize, Serialize};

pub const SHELL_SOCKET: &str = "/run/astro/shell.sock";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ShellFrame {
    pub node_id: String,
    /// Venue theme picked on B.O.S.S.: taproom, vector, bigsky, pinball, classic.
    pub theme: String,
    pub view: ShellView,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "screen", rename_all = "snake_case")]
pub enum ShellView {
    /// No session and no attract game on screen: call to action.
    Idle,
    /// Session started; its games are downloading.
    Staging {
        session_code: String,
        ready: u32,
        total: u32,
        time_remaining_sec: u32,
    },
    /// The player's game list.
    Picker {
        session_code: String,
        time_remaining_sec: u32,
        games: Vec<ShellGame>,
    },
    /// Time ran out; top-up window open.
    TimesUp {
        session_code: String,
        grace_left_sec: u32,
    },
    /// Session over, waiting for B.O.S.S. to start attract mode.
    Ended,
    /// Resident shell should drop DRM master so the emulator can take the display.
    Suspended,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ShellGame {
    pub game_id: String,
    pub title: String,
    pub system: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ShellAction {
    Launch { game_id: String },
    /// The player chose "Change my games" (B.O.S.S. is told).
    ChangeGames,
    /// Resident shell released DRM master and is ready for SIGSTOP.
    DisplayReleased,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_and_actions_are_tagged() {
        let f = ShellFrame {
            node_id: "ASTRO-01".into(),
            theme: "vector".into(),
            view: ShellView::TimesUp { session_code: "BRC-4KQ7".into(), grace_left_sec: 42 },
        };
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains(r#""screen":"times_up""#));
        assert_eq!(serde_json::from_str::<ShellFrame>(&json).unwrap(), f);
        let a: ShellAction = serde_json::from_str(r#"{"action":"launch","game_id":"dkc"}"#).unwrap();
        assert_eq!(a, ShellAction::Launch { game_id: "dkc".into() });

        let suspended = ShellFrame {
            node_id: "ASTRO-01".into(),
            theme: "taproom".into(),
            view: ShellView::Suspended,
        };
        let suspended_json = serde_json::to_string(&suspended).unwrap();
        assert!(suspended_json.contains(r#""screen":"suspended""#));
        assert_eq!(serde_json::from_str::<ShellFrame>(&suspended_json).unwrap(), suspended);

        let released_json = serde_json::to_string(&ShellAction::DisplayReleased).unwrap();
        assert_eq!(released_json, r#"{"action":"display_released"}"#);
        assert_eq!(
            serde_json::from_str::<ShellAction>(&released_json).unwrap(),
            ShellAction::DisplayReleased
        );
    }
}
