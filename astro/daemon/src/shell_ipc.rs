//! Serves the `astro-shell` customer screen over a Unix socket.
//!
//! The shell is the frontend service: it runs whenever no game is on screen.
//! Launching a game stops the service (freeing the display for RetroArch) and
//! the game's exit starts it again, so the shell holds no state of its own.

#[cfg(any(target_os = "linux", test))]
use crate::session;
#[cfg(target_os = "linux")]
use crate::staging;
use crate::DaemonSharedState;
#[cfg(target_os = "linux")]
use beercade_shared::shell::ShellAction;
#[cfg(any(target_os = "linux", test))]
use beercade_shared::shell::{ShellFrame, ShellGame, ShellView};
use std::sync::Arc;
use tokio::sync::RwLock;

/// What the shell should show for the current daemon state.
#[cfg(any(target_os = "linux", test))]
pub fn frame(st: &DaemonSharedState) -> ShellFrame {
    let view = match &st.session {
        Some(s) => match &s.phase {
            session::Phase::Staging => ShellView::Staging {
                session_code: s.code.clone(),
                ready: s.ready.len() as u32,
                total: s.games.len() as u32,
                time_remaining_sec: s.remaining,
            },
            // A game is (still) on screen; the shell is normally stopped then.
            session::Phase::Picker | session::Phase::InGame { .. } => ShellView::Picker {
                session_code: s.code.clone(),
                time_remaining_sec: s.remaining,
                games: s
                    .playable()
                    .into_iter()
                    .map(|g| ShellGame {
                        game_id: g.game_id.clone(),
                        title: g.title.clone(),
                        system: g.system.clone(),
                    })
                    .collect(),
            },
            session::Phase::Expired { grace_left, .. } => ShellView::TimesUp {
                session_code: s.code.clone(),
                grace_left_sec: *grace_left,
            },
            session::Phase::Ended => ShellView::Ended,
        },
        None => ShellView::Idle,
    };
    ShellFrame { node_id: st.node_id.clone(), theme: st.theme.clone(), view }
}

/// Act on what the player picked.
#[cfg(target_os = "linux")]
async fn handle(state_arc: &Arc<RwLock<DaemonSharedState>>, client: &reqwest::Client, action: ShellAction) {
    match action {
        ShellAction::Launch { game_id } => {
            let launch = {
                let st = state_arc.read().await;
                st.session
                    .as_ref()
                    .filter(|s| matches!(s.phase, session::Phase::Picker))
                    .and_then(|s| s.playable().into_iter().find(|g| g.game_id == game_id).cloned())
                    .and_then(|g| {
                        staging::game_path(&staging::session_dir(&st.data_dir), &g)
                            .map(|p| (g.core, p.to_string_lossy().to_string()))
                    })
            };
            match launch {
                Some((core, rom)) => {
                    let (state_arc, client) = (state_arc.clone(), client.clone());
                    tokio::spawn(async move {
                        crate::launch_game(&core, &rom, &false, &state_arc, &client).await;
                    });
                }
                None => tracing::warn!(game_id = %game_id, "Shell asked for a game that isn't playable"),
            }
        }
        ShellAction::ChangeGames => {
            let fx = state_arc
                .read()
                .await
                .session
                .as_ref()
                .filter(|s| s.is_live())
                .map(|s| s.change_requested())
                .unwrap_or_default();
            crate::apply_session_effects(state_arc, fx);
        }
        ShellAction::DisplayReleased => {
            // Handled by suspend handshake in Wave 3B.
        }
    }
}

#[cfg(target_os = "linux")]
pub fn spawn(state_arc: Arc<RwLock<DaemonSharedState>>, client: reqwest::Client) {
    use beercade_shared::shell::SHELL_SOCKET;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::time::{interval, Duration};

    let path = std::path::Path::new(SHELL_SOCKET);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::remove_file(path);
    let listener = match tokio::net::UnixListener::bind(path) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Shell socket {SHELL_SOCKET} unavailable: {e}");
            return;
        }
    };
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { continue };
            let (state_arc, client) = (state_arc.clone(), client.clone());
            tokio::spawn(async move {
                let (rd, mut wr) = stream.into_split();
                let mut lines = BufReader::new(rd).lines();
                let mut tick = interval(Duration::from_millis(500));
                let mut last = String::new();
                let mut beat = 0u32;
                loop {
                    tokio::select! {
                        line = lines.next_line() => match line {
                            Ok(Some(l)) => match serde_json::from_str::<ShellAction>(&l) {
                                Ok(a) => handle(&state_arc, &client, a).await,
                                Err(e) => tracing::warn!("Bad shell action {l:?}: {e}"),
                            },
                            _ => break,
                        },
                        _ = tick.tick() => {
                            let json = serde_json::to_string(&frame(&*state_arc.read().await)).unwrap_or_default();
                            beat += 1;
                            // Send on change, and every second for the clock.
                            if json != last || beat % 2 == 0 {
                                if wr.write_all(format!("{json}\n").as_bytes()).await.is_err() {
                                    break;
                                }
                                last = json;
                            }
                        }
                    }
                }
            });
        }
    });
}

#[cfg(not(target_os = "linux"))]
pub fn spawn(_state_arc: Arc<RwLock<DaemonSharedState>>, _client: reqwest::Client) {}

#[cfg(test)]
mod tests {
    use super::*;
    use beercade_shared::models::GameRef;

    fn g(id: &str) -> GameRef {
        GameRef {
            game_id: id.into(),
            title: id.to_uppercase(),
            system: "snes".into(),
            core: "snes9x".into(),
            file_name: format!("{id}.zip"),
            size_bytes: 1,
            sha256: String::new(),
            artwork_url: None,
        }
    }

    fn state() -> DaemonSharedState {
        crate::tests::sample_daemon_state(std::env::temp_dir().join("astro-shell-ipc/credits.json"))
    }

    #[test]
    fn idle_without_session() {
        assert_eq!(frame(&state()).view, ShellView::Idle);
    }

    #[test]
    fn picker_lists_only_ready_games() {
        let mut st = state();
        let (mut s, _) = session::Session::start("s", "BRC-1", 600, 90, 10, vec![g("a"), g("b")]);
        assert!(matches!(
            { st.session = Some(s.clone()); frame(&st).view },
            ShellView::Staging { ready: 0, total: 2, .. }
        ));
        s.staged(vec!["b".into()], vec!["a".into()]);
        st.session = Some(s);
        match frame(&st).view {
            ShellView::Picker { games, session_code, .. } => {
                assert_eq!(session_code, "BRC-1");
                assert_eq!(games.iter().map(|x| x.game_id.as_str()).collect::<Vec<_>>(), ["b"]);
            }
            v => panic!("{v:?}"),
        }
    }

    #[test]
    fn expired_and_ended() {
        let mut st = state();
        let (mut s, _) = session::Session::start("s", "C", 1, 5, 10, vec![]);
        s.tick();
        st.session = Some(s.clone());
        assert!(matches!(frame(&st).view, ShellView::TimesUp { grace_left_sec: 5, .. }));
        s.end();
        st.session = Some(s);
        assert_eq!(frame(&st).view, ShellView::Ended);
    }
}
