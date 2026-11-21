//! Serves the `astro-shell` customer screen over a Unix socket.
//!
//! The resident `astro-shell` keeps a persistent connection and receives instant
//! `watch<ShellFrame>` pushes whenever session, theme, or suspend state changes
//! (plus 1 s countdown updates while a timer is running). When a game launches,
//! the daemon pushes `ShellView::Suspended`, waits up to 500 ms for
//! `ShellAction::DisplayReleased`, and sends `SIGSTOP` to `astro-shell`; on
//! game exit it sends `SIGCONT` and pushes the active `ShellFrame`.

use crate::session;
#[cfg(target_os = "linux")]
use crate::staging;
use crate::DaemonSharedState;
#[cfg(target_os = "linux")]
use beercade_shared::shell::ShellAction;
use beercade_shared::shell::{ShellFrame, ShellGame, ShellView};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;
use tokio::sync::{watch, Notify, RwLock};

/// Reactive publisher driving all connected `astro-shell` clients via `watch`.
#[derive(Clone, Debug)]
pub struct ShellPublisher {
    tx: watch::Sender<ShellFrame>,
    suspended: Arc<AtomicBool>,
    client_pid: Arc<AtomicI32>,
    released_ack: Arc<AtomicBool>,
    released_notify: Arc<Notify>,
}

impl ShellPublisher {
    pub fn new(initial: ShellFrame) -> Self {
        let (tx, _) = watch::channel(initial);
        Self {
            tx,
            suspended: Arc::new(AtomicBool::new(false)),
            client_pid: Arc::new(AtomicI32::new(-1)),
            released_ack: Arc::new(AtomicBool::new(false)),
            released_notify: Arc::new(Notify::new()),
        }
    }

    pub fn notify(&self, frame: ShellFrame) {
        let next = if self.suspended.load(Ordering::Acquire) {
            ShellFrame {
                node_id: frame.node_id,
                theme: frame.theme,
                view: ShellView::Suspended,
            }
        } else {
            frame
        };
        self.tx.send_if_modified(|cur| {
            if *cur != next {
                *cur = next;
                true
            } else {
                false
            }
        });
    }

    pub fn notify_state(&self, st: &DaemonSharedState) {
        self.notify(frame(st));
    }

    pub fn is_suspended(&self) -> bool {
        self.suspended.load(Ordering::Acquire)
    }

    /// Push `ShellView::Suspended`, wait up to 500 ms for `DisplayReleased`,
    /// then `SIGSTOP` the resident `astro-shell` process.
    pub async fn suspend_for_game(&self, state_arc: &Arc<RwLock<DaemonSharedState>>) {
        self.suspended.store(true, Ordering::Release);
        self.released_ack.store(false, Ordering::Release);
        let (node_id, theme) = {
            let st = state_arc.read().await;
            (st.node_id.clone(), st.theme.clone())
        };
        let _ = self.tx.send(ShellFrame {
            node_id,
            theme,
            view: ShellView::Suspended,
        });

        let pid = self.client_pid.load(Ordering::Acquire);
        if pid > 0 {
            if !self.released_ack.load(Ordering::Acquire) {
                let notified = self.released_notify.notified();
                if !self.released_ack.load(Ordering::Acquire) {
                    let _ = tokio::time::timeout(std::time::Duration::from_millis(500), notified)
                        .await;
                }
            }
            let pid = self.client_pid.load(Ordering::Acquire);
            if pid > 0 {
                #[cfg(target_os = "linux")]
                // SAFETY: `pid` came from `SO_PEERCRED` on the local shell Unix socket.
                unsafe {
                    libc::kill(pid, libc::SIGSTOP);
                }
            }
        }
    }

    /// Send `SIGCONT` to the resident `astro-shell` and push the current frame.
    pub async fn resume_after_game(&self, state_arc: &Arc<RwLock<DaemonSharedState>>) {
        self.suspended.store(false, Ordering::Release);
        let pid = self.client_pid.load(Ordering::Acquire);
        if pid > 0 {
            #[cfg(target_os = "linux")]
            // SAFETY: `pid` came from `SO_PEERCRED` on the local shell Unix socket.
            unsafe {
                libc::kill(pid, libc::SIGCONT);
            }
        }
        let next = {
            let st = state_arc.read().await;
            frame(&st)
        };
        self.notify(next);
    }
}

/// What the shell should show for the current daemon state.
pub fn frame(st: &DaemonSharedState) -> ShellFrame {
    let view = match &st.session {
        Some(s) => match &s.phase {
            session::Phase::Staging => ShellView::Staging {
                session_code: s.code.clone(),
                ready: s.ready.len() as u32,
                total: s.games.len() as u32,
                time_remaining_sec: s.remaining,
            },
            // A game is (still) on screen; the shell is normally suspended then.
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
    ShellFrame {
        node_id: st.node_id.clone(),
        theme: st.theme.clone(),
        view,
    }
}

/// Act on what the player picked.
#[cfg(target_os = "linux")]
async fn handle(
    state_arc: &Arc<RwLock<DaemonSharedState>>,
    client: &reqwest::Client,
    action: ShellAction,
    publisher: &ShellPublisher,
) {
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
            publisher.released_ack.store(true, Ordering::Release);
            publisher.released_notify.notify_waiters();
        }
    }
}

#[cfg(target_os = "linux")]
pub fn spawn(
    state_arc: Arc<RwLock<DaemonSharedState>>,
    client: reqwest::Client,
    publisher: ShellPublisher,
) -> bool {
    use beercade_shared::shell::SHELL_SOCKET;
    use std::os::unix::fs::PermissionsExt;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let path = std::path::Path::new(SHELL_SOCKET);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::remove_file(path);
    let listener = match tokio::net::UnixListener::bind(path) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Shell socket {SHELL_SOCKET} unavailable: {e}");
            return false;
        }
    };
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660));

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { continue };
            let peer_pid = stream.peer_cred().ok().and_then(|c| c.pid()).unwrap_or(-1);
            if peer_pid > 0 {
                publisher.client_pid.store(peer_pid, Ordering::Release);
            }
            let (state_arc, client, publisher) =
                (state_arc.clone(), client.clone(), publisher.clone());
            tokio::spawn(async move {
                let (rd, mut wr) = stream.into_split();
                let mut lines = BufReader::new(rd).lines();
                let mut rx = publisher.tx.subscribe();

                let initial = rx.borrow_and_update().clone();
                let mut last = serde_json::to_string(&initial).unwrap_or_default();
                if !last.is_empty() && wr.write_all(format!("{last}\n").as_bytes()).await.is_err() {
                    if peer_pid > 0 {
                        let _ = publisher.client_pid.compare_exchange(
                            peer_pid,
                            -1,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        );
                    }
                    return;
                }

                loop {
                    tokio::select! {
                        line = lines.next_line() => match line {
                            Ok(Some(l)) => match serde_json::from_str::<ShellAction>(&l) {
                                Ok(a) => handle(&state_arc, &client, a, &publisher).await,
                                Err(e) => tracing::warn!("Bad shell action {l:?}: {e}"),
                            },
                            _ => break,
                        },
                        changed = rx.changed() => {
                            if changed.is_err() {
                                break;
                            }
                            let next = rx.borrow_and_update().clone();
                            let json = serde_json::to_string(&next).unwrap_or_default();
                            if json != last {
                                if wr.write_all(format!("{json}\n").as_bytes()).await.is_err() {
                                    break;
                                }
                                last = json;
                            }
                        }
                    }
                }
                if peer_pid > 0 {
                    let _ = publisher.client_pid.compare_exchange(
                        peer_pid,
                        -1,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    );
                }
            });
        }
    });
    true
}

#[cfg(not(target_os = "linux"))]
pub fn spawn(
    _state_arc: Arc<RwLock<DaemonSharedState>>,
    _client: reqwest::Client,
    _publisher: ShellPublisher,
) -> bool {
    false
}

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

    #[tokio::test]
    async fn publisher_notifies_watch_and_honors_suspend() {
        let st = state();
        let pub_handle = ShellPublisher::new(frame(&st));
        let mut rx = pub_handle.tx.subscribe();
        assert_eq!(rx.borrow().view, ShellView::Idle);

        let arc = Arc::new(RwLock::new(st));
        pub_handle.suspend_for_game(&arc).await;
        assert!(rx.has_changed().unwrap());
        assert_eq!(rx.borrow_and_update().view, ShellView::Suspended);

        // Normal updates while suspended remain Suspended.
        pub_handle.notify(ShellFrame {
            node_id: "test-node-1".into(),
            theme: "vector".into(),
            view: ShellView::Ended,
        });
        assert_eq!(rx.borrow_and_update().view, ShellView::Suspended);

        pub_handle.resume_after_game(&arc).await;
        assert_eq!(rx.borrow_and_update().view, ShellView::Idle);
    }
}
