//! Game files sent by B.O.S.S. for a session or attract playlist.
//!
//! Cabinets keep no games at rest. Files are fetched from
//! `GET /api/v1/cabinet/:node_id/games/:game_id/file` into `session/` or
//! `attract/` under the data dir, checked against the SHA-256 B.O.S.S. sent,
//! and wiped when the session or playlist ends and on every daemon start (so a
//! power cut can't leave a session's games behind).

use beercade_shared::models::GameRef;
use futures_util::{stream, StreamExt};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Downloads in flight per session; enough to overlap without starving play.
const CONCURRENCY: usize = 2;

pub fn session_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("session")
}

pub fn attract_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("attract")
}

/// Delete everything in `dir` and leave it empty. Missing is fine.
pub fn wipe(dir: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    std::fs::create_dir_all(dir)
}

/// The file name B.O.S.S. gave, if it is a plain name that stays inside the
/// staging dir.
pub fn safe_name(file_name: &str) -> Option<&str> {
    let ok = !file_name.is_empty()
        && !file_name.starts_with('.')
        && !file_name.contains(['/', '\\', '\0'])
        && file_name != "..";
    ok.then_some(file_name)
}

/// Where a staged game lives (whether or not it has been fetched yet).
pub fn game_path(dir: &Path, game: &GameRef) -> Option<PathBuf> {
    safe_name(&game.file_name).map(|n| dir.join(n))
}

#[derive(Debug)]
pub enum StageError {
    BadName,
    Http(String),
    Status(u16),
    Io(std::io::Error),
    HashMismatch,
}

impl std::fmt::Display for StageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StageError::BadName => write!(f, "unsafe file name"),
            StageError::Http(e) => write!(f, "download failed: {e}"),
            StageError::Status(s) => write!(f, "B.O.S.S. answered HTTP {s}"),
            StageError::Io(e) => write!(f, "disk: {e}"),
            StageError::HashMismatch => write!(f, "SHA-256 mismatch"),
        }
    }
}

impl From<std::io::Error> for StageError {
    fn from(e: std::io::Error) -> Self {
        StageError::Io(e)
    }
}

async fn file_sha256(path: &Path) -> std::io::Result<String> {
    let mut f = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[derive(Clone)]
pub struct Stager {
    client: reqwest::Client,
    boss_url: String,
    token: String,
    node_id: String,
}

impl Stager {
    pub fn new(boss_url: &str, token: &str, node_id: &str) -> Self {
        // No overall timeout: big titles take minutes. Stalls are caught by
        // the read timeout instead.
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .read_timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Stager {
            client,
            boss_url: boss_url.trim_end_matches('/').to_string(),
            token: token.to_string(),
            node_id: node_id.to_string(),
        }
    }

    fn url(&self, game: &GameRef) -> String {
        format!(
            "{}/api/v1/cabinet/{}/games/{}/file",
            self.boss_url, self.node_id, game.game_id
        )
    }

    /// Fetch one game into `dir`, resuming a partial download, and verify it.
    pub async fn fetch(&self, game: &GameRef, dir: &Path) -> Result<PathBuf, StageError> {
        let path = game_path(dir, game).ok_or(StageError::BadName)?;
        let want = game.sha256.to_ascii_lowercase();
        if path.exists() && file_sha256(&path).await? == want {
            return Ok(path);
        }
        tokio::fs::create_dir_all(dir).await?;
        let mut part = path.clone().into_os_string();
        part.push(".part");
        let part = PathBuf::from(part);

        let have = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);
        let mut req = self.client.get(self.url(game));
        if !self.token.is_empty() {
            req = req.bearer_auth(&self.token);
        }
        if have > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
        }
        let mut resp = req.send().await.map_err(|e| StageError::Http(e.to_string()))?;
        let status = resp.status().as_u16();
        let append = match status {
            206 => true,
            200 => false,
            s => return Err(StageError::Status(s)),
        };
        let mut out = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(&part)
            .await?;
        while let Some(chunk) = resp.chunk().await.map_err(|e| StageError::Http(e.to_string()))? {
            out.write_all(&chunk).await?;
        }
        out.sync_all().await?;
        drop(out);

        if file_sha256(&part).await? != want {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(StageError::HashMismatch);
        }
        tokio::fs::rename(&part, &path).await?;
        Ok(path)
    }

    /// Fetch every game; returns the ids that are ready and those that failed.
    pub async fn stage_all(&self, games: &[GameRef], dir: &Path) -> (Vec<String>, Vec<String>) {
        // Owned per download so the future stays Send for tokio::spawn.
        let jobs = games.iter().cloned().map(|g| (self.clone(), dir.to_path_buf(), g));
        let results: Vec<(String, Result<PathBuf, StageError>)> = stream::iter(jobs)
            .map(|(stager, dir, g)| async move {
                let r = stager.fetch(&g, &dir).await;
                (g.game_id, r)
            })
            .buffered(CONCURRENCY)
            .collect()
            .await;
        let mut ready = Vec::new();
        let mut failed = Vec::new();
        for (id, r) in results {
            match r {
                Ok(_) => ready.push(id),
                Err(e) => {
                    tracing::warn!(game_id = %id, "Staging failed: {e}");
                    failed.push(id);
                }
            }
        }
        (ready, failed)
    }
}

/// Remove staged files for games no longer on the list, keeping `keep`.
pub fn prune(dir: &Path, keep: &[GameRef]) -> std::io::Result<()> {
    let names: Vec<&str> = keep.iter().filter_map(|g| safe_name(&g.file_name)).collect();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for e in entries.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        let base = name.strip_suffix(".part").unwrap_or(&name);
        if !names.contains(&base) {
            std::fs::remove_file(e.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn game(id: &str, name: &str, body: &[u8]) -> GameRef {
        GameRef {
            game_id: id.into(),
            title: id.into(),
            system: "snes".into(),
            core: "snes9x".into(),
            file_name: name.into(),
            size_bytes: body.len() as u64,
            sha256: format!("{:x}", Sha256::digest(body)),
            artwork_url: None,
        }
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("astro-staging-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    /// One-shot HTTP server answering every request with `status` + `body`,
    /// honouring `Range: bytes=N-` with a 206. Returns the base URL.
    async fn serve(status: u16, body: Vec<u8>, requests: usize) -> String {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move {
            for _ in 0..requests {
                let (mut s, _) = l.accept().await.unwrap();
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.unwrap();
                let req = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                let from = req
                    .lines()
                    .find_map(|l| l.strip_prefix("range: bytes="))
                    .and_then(|r| r.trim().trim_end_matches('-').parse::<usize>().ok());
                let (code, data) = match (status, from) {
                    (200, Some(f)) => (206, body[f..].to_vec()),
                    _ => (status, body.clone()),
                };
                let head = format!(
                    "HTTP/1.1 {code} X\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    data.len()
                );
                s.write_all(head.as_bytes()).await.unwrap();
                s.write_all(&data).await.unwrap();
            }
        });
        url
    }

    #[test]
    fn rejects_unsafe_names() {
        assert!(safe_name("Donkey Kong Country.zip").is_some());
        for bad in ["", "..", "../etc/passwd", "a/b.zip", "a\\b.zip", ".hidden"] {
            assert!(safe_name(bad).is_none(), "{bad}");
        }
    }

    #[tokio::test]
    async fn fetches_and_verifies() {
        let body = b"SNES ROM BYTES".to_vec();
        let url = serve(200, body.clone(), 1).await;
        let dir = tmp("ok");
        let g = game("dkc", "dkc.zip", &body);
        let p = Stager::new(&url, "tok", "ASTRO-01").fetch(&g, &dir).await.unwrap();
        assert_eq!(std::fs::read(p).unwrap(), body);
    }

    #[tokio::test]
    async fn resumes_partial_download() {
        let body = b"0123456789ABCDEF".to_vec();
        let url = serve(200, body.clone(), 1).await;
        let dir = tmp("resume");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("g.zip.part"), &body[..6]).unwrap();
        let g = game("g", "g.zip", &body);
        let p = Stager::new(&url, "", "ASTRO-01").fetch(&g, &dir).await.unwrap();
        assert_eq!(std::fs::read(p).unwrap(), body);
    }

    #[tokio::test]
    async fn bad_hash_leaves_nothing() {
        let url = serve(200, b"tampered".to_vec(), 1).await;
        let dir = tmp("hash");
        let g = game("g", "g.zip", b"original");
        let err = Stager::new(&url, "", "ASTRO-01").fetch(&g, &dir).await.unwrap_err();
        assert!(matches!(err, StageError::HashMismatch));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn forbidden_is_reported_per_game() {
        let url = serve(403, Vec::new(), 2).await;
        let dir = tmp("403");
        let games = [game("a", "a.zip", b"a"), game("b", "b.zip", b"b")];
        let (ready, failed) = Stager::new(&url, "", "ASTRO-01").stage_all(&games, &dir).await;
        assert!(ready.is_empty());
        assert_eq!(failed, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn wipe_and_prune() {
        let dir = tmp("prune");
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["keep.zip", "drop.zip", "drop2.zip.part"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        prune(&dir, &[game("k", "keep.zip", b"x")]).unwrap();
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left, vec![std::ffi::OsString::from("keep.zip")]);
        wipe(&dir).unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    }
}
