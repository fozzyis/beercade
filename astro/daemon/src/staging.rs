//! Game files sent by B.O.S.S. for a session or attract playlist.
//!
//! Cabinets keep no patron state at rest. Session ROMs are fetched from
//! `GET /api/v1/cabinet/:node_id/games/:game_id/file` into `session/` or
//! `attract/` under the data dir, verified against the SHA-256 B.O.S.S. sent,
//! optionally served via hardlink from a bounded LRU cache (`cache/roms/`), and
//! wiped when the session or playlist ends and on every daemon start.

use beercade_shared::crypto::{compute_reader_sha256, verify_sha256_hex, Sha256Hasher};
use beercade_shared::models::GameRef;
use futures_util::{stream, StreamExt};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

/// Downloads in flight per session; enough to overlap without starving play.
const CONCURRENCY: usize = 2;

/// Default bounded ROM cache budget (4 GiB).
pub const DEFAULT_ROM_CACHE_MAX_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// Default minimum free disk space required before staging (256 MiB).
pub const DEFAULT_MIN_FREE_BYTES: u64 = 256 * 1024 * 1024;

static CACHE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn cache_lock() -> &'static Mutex<()> {
    CACHE_LOCK.get_or_init(|| Mutex::new(()))
}

#[derive(Clone, Debug)]
pub struct StageConfig {
    pub cache_dir: Option<PathBuf>,
    pub cache_max_bytes: u64,
    pub min_free_bytes: u64,
}

impl Default for StageConfig {
    fn default() -> Self {
        Self {
            cache_dir: None,
            cache_max_bytes: env_u64("ASTRO_ROM_CACHE_MAX_BYTES", DEFAULT_ROM_CACHE_MAX_BYTES),
            min_free_bytes: env_u64("ASTRO_MIN_FREE_BYTES", DEFAULT_MIN_FREE_BYTES),
        }
    }
}

impl StageConfig {
    pub fn from_data_dir(data_dir: &Path) -> Self {
        Self {
            cache_dir: Some(rom_cache_dir(data_dir)),
            cache_max_bytes: env_u64("ASTRO_ROM_CACHE_MAX_BYTES", DEFAULT_ROM_CACHE_MAX_BYTES),
            min_free_bytes: env_u64("ASTRO_MIN_FREE_BYTES", DEFAULT_MIN_FREE_BYTES),
        }
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

pub fn session_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("session")
}

pub fn attract_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("attract")
}

pub fn rom_cache_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("cache").join("roms")
}

/// Delete everything inside `dir` and leave `dir` itself intact and empty.
pub fn wipe(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for entry in std::fs::read_dir(dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        let path = entry.path();
        let res = if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        match res {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Wipe all inter-patron RetroArch artifacts (`saves`, `states`, `screenshots`)
/// and recreate the empty directories. Called on daemon boot and every session end.
pub async fn wipe_patron_artifacts(data_dir: &Path) {
    let data_dir = data_dir.to_path_buf();
    let _ = tokio::task::spawn_blocking(move || {
        for sub in ["saves", "states", "screenshots"] {
            let dir = data_dir.join(sub);
            if let Err(e) = wipe(&dir) {
                tracing::error!(dir = %dir.display(), "Failed to wipe patron artifacts: {e}");
            }
        }
    })
    .await;
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
    InsufficientSpace { needed: u64, available: u64 },
}

impl std::fmt::Display for StageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StageError::BadName => write!(f, "unsafe file name"),
            StageError::Http(e) => write!(f, "download failed: {e}"),
            StageError::Status(s) => write!(f, "B.O.S.S. answered HTTP {s}"),
            StageError::Io(e) => write!(f, "disk: {e}"),
            StageError::HashMismatch => write!(f, "SHA-256 mismatch"),
            StageError::InsufficientSpace { needed, available } => {
                write!(f, "insufficient disk space: need {needed} bytes, {available} available")
            }
        }
    }
}

impl From<std::io::Error> for StageError {
    fn from(e: std::io::Error) -> Self {
        StageError::Io(e)
    }
}

async fn file_sha256(path: &Path) -> std::io::Result<String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let f = std::fs::File::open(path)?;
        compute_reader_sha256(f)
    })
    .await
    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
}

fn is_valid_sha256_key(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn cache_entry_path(cache_dir: &Path, key: &str) -> PathBuf {
    cache_dir.join(format!("{key}.rom"))
}

fn lru_index_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(".lru")
}

fn load_lru_keys(cache_dir: &Path) -> Vec<String> {
    let raw = std::fs::read_to_string(lru_index_path(cache_dir)).unwrap_or_default();
    let mut keys = Vec::new();
    let mut seen = HashSet::new();
    for line in raw.lines() {
        let k = line.trim();
        if is_valid_sha256_key(k) && cache_entry_path(cache_dir, k).is_file() && seen.insert(k.to_string()) {
            keys.push(k.to_string());
        }
    }
    if let Ok(entries) = std::fs::read_dir(cache_dir) {
        let mut extras: Vec<String> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name();
                let s = name.to_str()?;
                let stem = s.strip_suffix(".rom")?;
                (is_valid_sha256_key(stem) && !seen.contains(stem)).then(|| stem.to_string())
            })
            .collect();
        extras.sort();
        for k in extras {
            seen.insert(k.clone());
            keys.push(k);
        }
    }
    keys
}

fn save_lru_keys(cache_dir: &Path, keys: &[String]) {
    let tmp = cache_dir.join(".lru.tmp");
    let body = keys.join("\n");
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, lru_index_path(cache_dir));
    }
}

fn link_or_copy(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if dst.exists() {
        let _ = std::fs::remove_file(dst);
    }
    match std::fs::hard_link(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(src, dst)?;
            Ok(())
        }
    }
}

fn try_cache_hit_sync(cache_dir: &Path, key: &str, dest_path: &Path) -> bool {
    if !is_valid_sha256_key(key) {
        return false;
    }
    let _guard = cache_lock().lock().unwrap_or_else(|e| e.into_inner());
    let entry = cache_entry_path(cache_dir, key);
    if !entry.is_file() {
        return false;
    }
    let ok = std::fs::File::open(&entry)
        .and_then(compute_reader_sha256)
        .is_ok_and(|actual| verify_sha256_hex(&actual, key));
    if !ok {
        let _ = std::fs::remove_file(&entry);
        let mut keys = load_lru_keys(cache_dir);
        keys.retain(|k| k != key);
        save_lru_keys(cache_dir, &keys);
        return false;
    }
    if link_or_copy(&entry, dest_path).is_err() {
        return false;
    }
    let mut keys = load_lru_keys(cache_dir);
    keys.retain(|k| k != key);
    keys.push(key.to_string());
    save_lru_keys(cache_dir, &keys);
    true
}

fn insert_cache_sync(cache_dir: &Path, key: &str, staged_path: &Path, max_bytes: u64) {
    if max_bytes == 0 || !is_valid_sha256_key(key) {
        return;
    }
    let Ok(meta) = std::fs::metadata(staged_path) else { return };
    let size = meta.len();
    if size > max_bytes {
        return;
    }
    let _guard = cache_lock().lock().unwrap_or_else(|e| e.into_inner());
    if std::fs::create_dir_all(cache_dir).is_err() {
        return;
    }
    let mut keys = load_lru_keys(cache_dir);
    keys.retain(|k| k != key);

    let mut used: u64 = keys
        .iter()
        .filter_map(|k| std::fs::metadata(cache_entry_path(cache_dir, k)).ok().map(|m| m.len()))
        .sum();

    let existing = cache_entry_path(cache_dir, key);
    if existing.is_file() {
        let _ = std::fs::remove_file(&existing);
    }

    while used.saturating_add(size) > max_bytes && !keys.is_empty() {
        let oldest = keys.remove(0);
        let p = cache_entry_path(cache_dir, &oldest);
        if let Ok(m) = std::fs::metadata(&p) {
            used = used.saturating_sub(m.len());
        }
        let _ = std::fs::remove_file(&p);
    }

    let tmp = cache_dir.join(format!("{key}.tmp"));
    if link_or_copy(staged_path, &tmp).is_ok() {
        if std::fs::rename(&tmp, &existing).is_ok() {
            keys.push(key.to_string());
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
    }
    save_lru_keys(cache_dir, &keys);
}

#[cfg(target_os = "linux")]
fn free_bytes_at(path: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let mut probe = path;
    while !probe.exists() {
        match probe.parent() {
            Some(p) if !p.as_os_str().is_empty() => probe = p,
            _ => break,
        }
    }
    let c_path = CString::new(probe.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c_path` is NUL-terminated and `stat` is a writable statvfs struct.
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    Some((stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64))
}

#[cfg(not(target_os = "linux"))]
fn free_bytes_at(_path: &Path) -> Option<u64> {
    None
}

fn ensure_free_space_sync(
    target_dir: &Path,
    cache_dir: Option<&Path>,
    file_bytes: u64,
    min_free_bytes: u64,
) -> Result<(), StageError> {
    let needed = min_free_bytes.saturating_add(file_bytes);
    let Some(mut avail) = free_bytes_at(target_dir) else {
        return Ok(());
    };
    if avail >= needed {
        return Ok(());
    }
    if let Some(cdir) = cache_dir {
        let _guard = cache_lock().lock().unwrap_or_else(|e| e.into_inner());
        let mut keys = load_lru_keys(cdir);
        while avail < needed && !keys.is_empty() {
            let oldest = keys.remove(0);
            let _ = std::fs::remove_file(cache_entry_path(cdir, &oldest));
            avail = free_bytes_at(target_dir).unwrap_or(avail);
        }
        save_lru_keys(cdir, &keys);
    }
    if avail < needed {
        return Err(StageError::InsufficientSpace { needed, available: avail });
    }
    Ok(())
}

#[derive(Clone)]
pub struct Stager {
    client: reqwest::Client,
    boss_url: String,
    token: String,
    node_id: String,
    config: StageConfig,
}

impl Stager {
    #[allow(dead_code)]
    pub fn new(boss_url: &str, token: &str, node_id: &str) -> Self {
        // No overall timeout: big titles take minutes. Stalls are caught by
        // the read timeout instead.
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .read_timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self::with_client_and_config(client, boss_url, token, node_id, StageConfig::default())
    }

    pub fn with_client_and_config(
        client: reqwest::Client,
        boss_url: &str,
        token: &str,
        node_id: &str,
        config: StageConfig,
    ) -> Self {
        Stager {
            client,
            boss_url: boss_url.trim_end_matches('/').to_string(),
            token: token.to_string(),
            node_id: node_id.to_string(),
            config,
        }
    }

    fn url(&self, game: &GameRef) -> String {
        format!(
            "{}/api/v1/cabinet/{}/games/{}/file",
            self.boss_url, self.node_id, game.game_id
        )
    }

    /// Fetch one game into `dir`, checking the bounded LRU ROM cache first,
    /// resuming partial downloads on HTTP 206, and verifying SHA-256.
    pub async fn fetch(&self, game: &GameRef, dir: &Path) -> Result<PathBuf, StageError> {
        let path = game_path(dir, game).ok_or(StageError::BadName)?;
        let want = game.sha256.trim().to_ascii_lowercase();
        if path.exists() {
            if let Ok(actual) = file_sha256(&path).await {
                if verify_sha256_hex(&actual, &want) || actual == want {
                    return Ok(path);
                }
            }
        }

        if let Some(cdir) = &self.config.cache_dir {
            let cdir_owned = cdir.clone();
            let want_owned = want.clone();
            let path_owned = path.clone();
            let hit = tokio::task::spawn_blocking(move || {
                try_cache_hit_sync(&cdir_owned, &want_owned, &path_owned)
            })
            .await
            .unwrap_or(false);
            if hit {
                return Ok(path);
            }
        }

        tokio::fs::create_dir_all(dir).await?;
        {
            let dir_owned = dir.to_path_buf();
            let cdir_owned = self.config.cache_dir.clone();
            let min_free = self.config.min_free_bytes;
            let expected = game.size_bytes;
            tokio::task::spawn_blocking(move || {
                ensure_free_space_sync(&dir_owned, cdir_owned.as_deref(), expected, min_free)
            })
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))??;
        }

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

        if let Some(remaining_len) = resp.content_length() {
            let dir_owned = dir.to_path_buf();
            let cdir_owned = self.config.cache_dir.clone();
            let min_free = self.config.min_free_bytes;
            tokio::task::spawn_blocking(move || {
                ensure_free_space_sync(&dir_owned, cdir_owned.as_deref(), remaining_len, min_free)
            })
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))??;
        }

        let mut out = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(&part)
            .await?;

        let mut hasher = (!append).then(Sha256Hasher::new);
        while let Some(chunk) = resp.chunk().await.map_err(|e| StageError::Http(e.to_string()))? {
            out.write_all(&chunk).await?;
            if let Some(h) = hasher.as_mut() {
                h.update(&chunk);
            }
        }
        out.sync_all().await?;
        drop(out);

        let hash_ok = if let Some(h) = hasher {
            h.verify_hex(&want)
        } else {
            let actual = file_sha256(&part).await?;
            verify_sha256_hex(&actual, &want)
        };
        if !hash_ok {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(StageError::HashMismatch);
        }
        tokio::fs::rename(&part, &path).await?;

        if let Some(cdir) = &self.config.cache_dir {
            let cdir_owned = cdir.clone();
            let want_owned = want.clone();
            let path_owned = path.clone();
            let max_bytes = self.config.cache_max_bytes;
            let _ = tokio::task::spawn_blocking(move || {
                insert_cache_sync(&cdir_owned, &want_owned, &path_owned, max_bytes);
            })
            .await;
        }

        Ok(path)
    }

    /// Fetch games concurrently and invoke `on_each` as each individual game finishes.
    pub async fn stage_streaming<F, Fut>(&self, games: &[GameRef], dir: &Path, mut on_each: F)
    where
        F: FnMut(String, Result<PathBuf, StageError>) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        let jobs = games.iter().cloned().map(|g| (self.clone(), dir.to_path_buf(), g));
        let mut s = stream::iter(jobs)
            .map(|(stager, dir, g)| async move {
                let r = stager.fetch(&g, &dir).await;
                (g.game_id, r)
            })
            .buffer_unordered(CONCURRENCY);
        while let Some((id, res)) = s.next().await {
            on_each(id, res).await;
        }
    }

    /// Fetch every game; returns the ids that are ready and those that failed.
    pub async fn stage_all(&self, games: &[GameRef], dir: &Path) -> (Vec<String>, Vec<String>) {
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
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
            sha256: beercade_shared::crypto::compute_sha256(body),
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
        let _ = std::fs::remove_dir_all(&dir);
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
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn bad_hash_leaves_nothing() {
        let url = serve(200, b"tampered".to_vec(), 1).await;
        let dir = tmp("hash");
        let g = game("g", "g.zip", b"original");
        let err = Stager::new(&url, "", "ASTRO-01").fetch(&g, &dir).await.unwrap_err();
        assert!(matches!(err, StageError::HashMismatch));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn forbidden_is_reported_per_game() {
        let url = serve(403, Vec::new(), 2).await;
        let dir = tmp("403");
        let games = [game("a", "a.zip", b"a"), game("b", "b.zip", b"b")];
        let (ready, failed) = Stager::new(&url, "", "ASTRO-01").stage_all(&games, &dir).await;
        assert!(ready.is_empty());
        assert_eq!(failed, vec!["a".to_string(), "b".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
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
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn lru_cache_hit_skips_http_and_evicts_oldest() {
        let root = tmp("lru");
        let cache_dir = root.join("cache/roms");
        let s1 = root.join("s1");
        let s2 = root.join("s2");

        let rom_a = vec![b'A'; 40];
        let rom_b = vec![b'B'; 40];
        let rom_c = vec![b'C'; 50];
        let g_a = game("a", "a.zip", &rom_a);
        let g_b = game("b", "b.zip", &rom_b);
        let g_c = game("c", "c.zip", &rom_c);

        let hits = Arc::new(AtomicUsize::new(0));
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let hits_srv = hits.clone();
        let (ra, rb, rc) = (rom_a.clone(), rom_b.clone(), rom_c.clone());
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { break };
                hits_srv.fetch_add(1, Ordering::SeqCst);
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.unwrap();
                let req = String::from_utf8_lossy(&buf[..n]);
                let body = if req.contains("/games/a/") {
                    &ra
                } else if req.contains("/games/b/") {
                    &rb
                } else {
                    &rc
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                s.write_all(head.as_bytes()).await.unwrap();
                s.write_all(body).await.unwrap();
            }
        });

        let cfg = StageConfig {
            cache_dir: Some(cache_dir.clone()),
            cache_max_bytes: 90,
            min_free_bytes: 0,
        };
        let stager = Stager::with_client_and_config(
            reqwest::Client::new(),
            &url,
            "",
            "ASTRO-01",
            cfg,
        );

        // Fetch A (40B) and B (40B) -> cache holds [A, B] = 80B <= 90B.
        stager.fetch(&g_a, &s1).await.unwrap();
        stager.fetch(&g_b, &s1).await.unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 2);

        // Fetch A into s2 -> cache hit! Zero HTTP transfer, A becomes MRU ([B, A]).
        let p_a2 = stager.fetch(&g_a, &s2).await.unwrap();
        assert_eq!(std::fs::read(p_a2).unwrap(), rom_a);
        assert_eq!(hits.load(Ordering::SeqCst), 2);

        // Fetch C (50B) -> 80 + 50 = 130 > 90 -> evicts oldest (B), keeps A + C (90B).
        stager.fetch(&g_c, &s2).await.unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 3);
        assert!(!cache_entry_path(&cache_dir, &g_b.sha256).exists(), "oldest B evicted");
        assert!(cache_entry_path(&cache_dir, &g_a.sha256).exists(), "MRU A retained");
        assert!(cache_entry_path(&cache_dir, &g_c.sha256).exists(), "new C cached");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn wipe_patron_artifacts_clears_saves_states_screenshots() {
        let root = tmp("patron-wipe");
        for sub in ["saves", "states", "screenshots", "session"] {
            let d = root.join(sub);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("patron.dat"), b"secret").unwrap();
        }
        wipe_patron_artifacts(&root).await;
        wipe(&session_dir(&root)).unwrap();
        for sub in ["saves", "states", "screenshots", "session"] {
            let d = root.join(sub);
            assert!(d.is_dir(), "{sub} dir recreated");
            assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0, "{sub} wiped");
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
