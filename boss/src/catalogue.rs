//! Game Catalogue & Attract Mode Scheduler for B.O.S.S.
//!
//! Tracks available titles, hashes, systems, libretro cores, and disk paths.
//! Selects 2-3 games from distinct systems for cabinet attract mode.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use beercade_shared::crypto::compute_reader_sha256;
use beercade_shared::models::GameRef;

/// Supported ROM file extensions (lowercase, without leading dot).
const SUPPORTED_ROM_EXTENSIONS: &[&str] = &[
    "zip", "7z", "chd", "iso", "bin", "cue", "sfc", "smc", "md", "gen",
    "gba", "n64", "z64", "v64", "nes", "pce", "pbp",
];

/// Returns the configured ROM library root (`BOSS_ROMS_DIR` or `/data/beercade/roms`).
pub fn roms_dir() -> PathBuf {
    std::env::var("BOSS_ROMS_DIR")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/data/beercade/roms"))
}

/// Returns the configured persistent catalogue JSON path (`BOSS_CATALOGUE_FILE` or `/data/beercade/catalogue.json`).
pub fn catalogue_file_path() -> PathBuf {
    std::env::var("BOSS_CATALOGUE_FILE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/data/beercade/catalogue.json"))
}

fn active_catalogue_file_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var("BOSS_CATALOGUE_FILE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        return Some(PathBuf::from(p));
    }
    if cfg!(test) {
        None
    } else {
        Some(catalogue_file_path())
    }
}

/// Catalogue entry pairing game metadata with its enabled/disabled status.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub game: GameRef,
    pub enabled: bool,
}

/// Compute lower-case hex SHA-256 for a file on disk without loading it into RAM.
pub fn compute_sha256<P: AsRef<Path>>(path: P) -> io::Result<String> {
    let file = File::open(path)?;
    compute_reader_sha256(file)
}

/// Map a system directory name (and optional extension fallback) to its default libretro core.
pub fn default_core_for_system(system: &str, ext: Option<&str>) -> &'static str {
    match system.trim().to_ascii_lowercase().as_str() {
        "snes" | "sfc" => "snes9x",
        "arcade" | "fbneo" | "neogeo" | "cps1" | "cps2" | "cps3" => "fbneo",
        "mame" => "mame2003_plus",
        "naomi" | "dreamcast" | "dc" | "atomiswave" => "flycast",
        "megadrive" | "genesis" | "md" | "gen" | "sms" | "gamegear" => "genesis_plus_gx",
        "gba" => "mgba",
        "gb" | "gbc" => "gambatte",
        "nes" | "famicom" => "nestopia",
        "n64" => "mupen64plus_next",
        "psx" | "ps1" | "playstation" => "pcsx_rearmed",
        "psp" => "ppsspp",
        "pce" | "pcengine" | "tg16" => "mednafen_pce_fast",
        "pc" | "dos" => "dosbox_pure",
        _ => match ext.map(|e| e.to_ascii_lowercase()).as_deref() {
            Some("sfc" | "smc") => "snes9x",
            Some("md" | "gen") => "genesis_plus_gx",
            Some("gba") => "mgba",
            Some("n64" | "z64" | "v64") => "mupen64plus_next",
            Some("nes") => "nestopia",
            Some("pce") => "mednafen_pce_fast",
            Some("pbp") => "ppsspp",
            Some("iso" | "bin" | "cue" | "chd") => "pcsx_rearmed",
            _ => "fbneo",
        },
    }
}

fn infer_system_from_ext(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "sfc" | "smc" => "snes",
        "md" | "gen" => "megadrive",
        "gba" => "gba",
        "n64" | "z64" | "v64" => "n64",
        "nes" => "nes",
        "pce" => "pce",
        "pbp" => "psp",
        "iso" | "bin" | "cue" | "chd" => "psx",
        _ => "arcade",
    }
}

fn is_supported_rom_ext(ext: &str) -> bool {
    let lower = ext.to_ascii_lowercase();
    SUPPORTED_ROM_EXTENSIONS.contains(&lower.as_str())
}

fn build_game_ref_from_rom(path: &Path, system: &str, core: &str) -> io::Result<GameRef> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid file name"))?
        .to_string();
    let metadata = std::fs::metadata(path)?;
    let size_bytes = metadata.len();
    let sha256 = compute_sha256(path)?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&file_name);
    let game_id = stem.to_lowercase().replace(' ', "_");
    let title = stem.to_string();

    Ok(GameRef {
        game_id,
        title,
        system: system.to_string(),
        core: core.to_string(),
        file_name,
        size_bytes,
        sha256,
        artwork_url: None,
    })
}

/// Walk `<dir>/<system>/*` (and any top-level ROM files in `<dir>`) and hash supported ROMs.
pub fn collect_roms_from_dir(dir: &Path) -> io::Result<Vec<GameRef>> {
    if !dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("ROM directory does not exist: {}", dir.display()),
        ));
    }

    let mut games = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let system = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("arcade")
                .to_ascii_lowercase();
            let Ok(sub_rd) = std::fs::read_dir(&path) else {
                continue;
            };
            for sub_entry in sub_rd.flatten() {
                let rom_path = sub_entry.path();
                if !rom_path.is_file() {
                    continue;
                }
                let Some(ext) = rom_path.extension().and_then(|e| e.to_str()) else {
                    continue;
                };
                if !is_supported_rom_ext(ext) {
                    continue;
                }
                let core = default_core_for_system(&system, Some(ext));
                if let Ok(game) = build_game_ref_from_rom(&rom_path, &system, core) {
                    games.push(game);
                }
            }
        } else if path.is_file() {
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if !is_supported_rom_ext(ext) {
                continue;
            }
            let system = infer_system_from_ext(ext);
            let core = default_core_for_system(system, Some(ext));
            if let Ok(game) = build_game_ref_from_rom(&path, system, core) {
                games.push(game);
            }
        }
    }

    games.sort_by(|a, b| a.system.cmp(&b.system).then_with(|| a.title.cmp(&b.title)));
    Ok(games)
}

/// In-memory game catalogue store with seed catalog and attract mode selection.
#[derive(Debug, Clone)]
pub struct GameCatalogue {
    entries: HashMap<String, CatalogEntry>,
}

impl Default for GameCatalogue {
    fn default() -> Self {
        Self::new()
    }
}

impl GameCatalogue {
    /// Create catalogue initialized with the default seeded games across SNES, Arcade, and Naomi.
    pub fn new() -> Self {
        let mut cat = Self {
            entries: HashMap::new(),
        };

        // Seeded SNES titles
        cat.insert(
            GameRef {
                game_id: "dkc".into(),
                title: "Donkey Kong Country".into(),
                system: "snes".into(),
                core: "snes9x".into(),
                file_name: "Donkey Kong Country.zip".into(),
                size_bytes: 2514751,
                sha256: "71000d2ba67d01ffe77112a85e32bc6247241da2212c272316ee5c1e938775f3".into(),
                artwork_url: None,
            },
            true,
        );

        cat.insert(
            GameRef {
                game_id: "dkc3".into(),
                title: "Donkey Kong Country 3 - Dixie Kong's Double Trouble".into(),
                system: "snes".into(),
                core: "snes9x".into(),
                file_name: "Donkey Kong Country 3 - Dixie Kong's Double Trouble.zip".into(),
                size_bytes: 3167855,
                sha256: "77145ffd8a09171838297063a2ce58bc91119c3dfcf645c34ce851cd14273862".into(),
                artwork_url: None,
            },
            true,
        );

        cat.insert(
            GameRef {
                game_id: "batman_returns".into(),
                title: "Batman Returns".into(),
                system: "snes".into(),
                core: "snes9x".into(),
                file_name: "Batman Returns.zip".into(),
                size_bytes: 758142,
                sha256: "c49a290ebf4b60730997a3ffd1347a06cf982479dbb529d3a1d02c3528a3c717".into(),
                artwork_url: None,
            },
            true,
        );

        cat.insert(
            GameRef {
                game_id: "bubsy2".into(),
                title: "Bubsy II".into(),
                system: "snes".into(),
                core: "snes9x".into(),
                file_name: "Bubsy II.zip".into(),
                size_bytes: 1147759,
                sha256: "8c2a330895b73b7aa765ac7e4cc5c927b050c499d41a7de6fbb188a42aacc6f3".into(),
                artwork_url: None,
            },
            true,
        );

        // Seeded Arcade titles
        cat.insert(
            GameRef {
                game_id: "sfiii3".into(),
                title: "Street Fighter III: 3rd Strike".into(),
                system: "arcade".into(),
                core: "fbneo".into(),
                file_name: "sfiii3.zip".into(),
                size_bytes: 68500000,
                sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
                artwork_url: None,
            },
            true,
        );

        cat.insert(
            GameRef {
                game_id: "mslugx".into(),
                title: "Metal Slug X".into(),
                system: "arcade".into(),
                core: "fbneo".into(),
                file_name: "mslugX.zip".into(),
                size_bytes: 31000000,
                sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
                artwork_url: None,
            },
            true,
        );

        // Seeded Naomi title
        cat.insert(
            GameRef {
                game_id: "mvc2".into(),
                title: "Marvel vs. Capcom 2".into(),
                system: "naomi".into(),
                core: "flycast".into(),
                file_name: "mvc2.zip".into(),
                size_bytes: 198000000,
                sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
                artwork_url: None,
            },
            true,
        );

        cat
    }

    /// Add or update a game entry in the catalogue.
    pub fn insert(&mut self, game: GameRef, enabled: bool) {
        self.entries
            .insert(game.game_id.clone(), CatalogEntry { game, enabled });
    }

    /// Retrieve an entry by game_id.
    pub fn get(&self, game_id: &str) -> Option<&CatalogEntry> {
        self.entries.get(game_id)
    }

    /// List all enabled games, sorted alphabetically by title.
    pub fn list_enabled(&self) -> Vec<GameRef> {
        let mut list: Vec<GameRef> = self
            .entries
            .values()
            .filter(|e| e.enabled)
            .map(|e| e.game.clone())
            .collect();
        list.sort_by(|a, b| a.title.cmp(&b.title));
        list
    }

    /// List all catalogue entries, sorted alphabetically by title.
    pub fn list_all(&self) -> Vec<CatalogEntry> {
        let mut list: Vec<CatalogEntry> = self.entries.values().cloned().collect();
        list.sort_by(|a, b| a.game.title.cmp(&b.game.title));
        list
    }

    /// Total number of games in the catalogue.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Number of enabled games in the catalogue.
    pub fn enabled_count(&self) -> usize {
        self.entries.values().filter(|e| e.enabled).count()
    }

    /// Enable or disable a game. Returns true if the game was found.
    pub fn set_enabled(&mut self, game_id: &str, enabled: bool) -> bool {
        if let Some(entry) = self.entries.get_mut(game_id) {
            entry.enabled = enabled;
            true
        } else {
            false
        }
    }

    /// Atomically persist the catalogue to disk (`tmp` -> `sync_all` -> `rename`).
    pub fn save_to_file(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let entries = self.list_all();
        let data = serde_json::to_vec_pretty(&entries)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let tmp_name = format!(
            ".{}.tmp.{}",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("catalogue.json"),
            uuid::Uuid::new_v4()
        );
        let tmp_path = path.with_file_name(tmp_name);
        {
            let mut f = File::create(&tmp_path)?;
            f.write_all(&data)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }

    /// Persist to `BOSS_CATALOGUE_FILE` (or `/data/beercade/catalogue.json`).
    pub fn save_to_default(&self) -> io::Result<()> {
        if let Some(path) = active_catalogue_file_path() {
            self.save_to_file(&path)?;
        }
        Ok(())
    }

    /// Load a catalogue snapshot from disk.
    pub fn load_from_file(path: &Path) -> io::Result<Self> {
        let bytes = std::fs::read(path)?;
        let entries_vec: Vec<CatalogEntry> = serde_json::from_slice(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut entries = HashMap::with_capacity(entries_vec.len());
        for entry in entries_vec {
            entries.insert(entry.game.game_id.clone(), entry);
        }
        Ok(Self { entries })
    }

    /// Load from `path` if present and non-empty (merging missing seeded entries
    /// as disabled fallback); otherwise return the seeded defaults.
    pub fn load_or_seed(path: &Path) -> Self {
        if path.is_file() {
            match Self::load_from_file(path) {
                Ok(mut loaded) if !loaded.entries.is_empty() => {
                    let seeded = Self::new();
                    for (id, entry) in seeded.entries {
                        loaded.entries.entry(id).or_insert(CatalogEntry {
                            game: entry.game,
                            enabled: false,
                        });
                    }
                    return loaded;
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "failed to load catalogue file; falling back to defaults");
                }
            }
        }
        Self::new()
    }

    /// Load from `BOSS_CATALOGUE_FILE` (or `/data/beercade/catalogue.json`), falling back to seeded defaults.
    pub fn load_or_default() -> Self {
        match active_catalogue_file_path() {
            Some(path) => Self::load_or_seed(&path),
            None => Self::new(),
        }
    }

    /// Merge scanned ROMs into the catalogue without overwriting existing `enabled` flags.
    pub fn merge_scanned_games(&mut self, scanned: Vec<GameRef>) -> usize {
        let count = scanned.len();
        for game in scanned {
            let matched_key = if self.entries.contains_key(&game.game_id) {
                Some(game.game_id.clone())
            } else {
                self.entries
                    .iter()
                    .find(|(_, e)| {
                        e.game.system.eq_ignore_ascii_case(&game.system)
                            && e.game.file_name.eq_ignore_ascii_case(&game.file_name)
                    })
                    .map(|(k, _)| k.clone())
            };

            if let Some(key) = matched_key {
                if let Some(existing) = self.entries.get_mut(&key) {
                    existing.game.sha256 = game.sha256;
                    existing.game.size_bytes = game.size_bytes;
                    existing.game.file_name = game.file_name;
                }
            } else {
                self.insert(game, true);
            }
        }
        count
    }

    /// Scan `<dir>/<system>/*` (and top-level ROMs) synchronously, merging into `self`
    /// while preserving existing `enabled` flags.
    pub fn scan_roms_dir(&mut self, dir: &Path) -> io::Result<usize> {
        let games = collect_roms_from_dir(dir)?;
        Ok(self.merge_scanned_games(games))
    }

    /// Import a ROM file from disk by hashing it with SHA-256 and measuring its size.
    pub fn import_rom_file(&mut self, path: &Path, system: &str, core: &str) -> io::Result<GameRef> {
        let game = build_game_ref_from_rom(path, system, core)?;
        self.insert(game.clone(), true);
        Ok(game)
    }

    /// Resolve a ROM file path on disk across standard B.E.E.R.C.A.D.E. storage paths.
    ///
    /// Checks:
    /// - `BOSS_ROMS_DIR` env variable (direct and immediate `<system>` subdirectories)
    /// - `/data/beercade/games/{file_name}`
    /// - `/data/beercade/roms/{file_name}` and `/data/beercade/roms/*/{file_name}`
    /// - fallback `astro/roms/{file_name}` and `../astro/roms/{file_name}`
    pub fn resolve_rom_path(&self, file_name: &str) -> Option<PathBuf> {
        let mut candidates = Vec::new();
        if let Ok(env_dir) = std::env::var("BOSS_ROMS_DIR") {
            let trimmed = env_dir.trim();
            if !trimmed.is_empty() {
                candidates.push(PathBuf::from(trimmed));
            }
        }
        candidates.push(PathBuf::from("/data/beercade/games"));
        candidates.push(PathBuf::from("/data/beercade/roms"));
        candidates.push(PathBuf::from("/data/beercade/roms/snes"));
        candidates.push(PathBuf::from("astro/roms"));
        candidates.push(PathBuf::from("../astro/roms"));

        for base in &candidates {
            let candidate = base.join(file_name);
            if candidate.is_file() {
                return Some(candidate);
            }
            if let Ok(rd) = std::fs::read_dir(base) {
                for entry in rd.flatten() {
                    let sub = entry.path();
                    if sub.is_dir() {
                        let nested = sub.join(file_name);
                        if nested.is_file() {
                            return Some(nested);
                        }
                    }
                }
            }
        }
        None
    }

    /// Pick 2-3 games from **different systems** for attract mode, avoiding games
    /// from the node's previous playlist if possible.
    pub fn pick_attract_games(&self, recent_game_ids: &[String]) -> Vec<GameRef> {
        let mut by_system: HashMap<String, Vec<GameRef>> = HashMap::new();
        for entry in self.entries.values() {
            if entry.enabled {
                by_system
                    .entry(entry.game.system.clone())
                    .or_default()
                    .push(entry.game.clone());
            }
        }

        if by_system.is_empty() {
            return Vec::new();
        }

        // Sort systems deterministically
        let mut systems: Vec<String> = by_system.keys().cloned().collect();
        systems.sort();

        let recent_set: HashSet<&str> = recent_game_ids.iter().map(|s| s.as_str()).collect();

        // Target 2-3 games from distinct systems (up to 3 if >=3 systems exist)
        let target_count = systems.len().min(3);

        // Sort candidate systems: prefer systems that have games not recently played
        systems.sort_by_key(|sys| {
            let games = &by_system[sys];
            let has_fresh = games.iter().any(|g| !recent_set.contains(g.game_id.as_str()));
            if has_fresh { 0 } else { 1 }
        });

        let selected_systems = &systems[..target_count];
        let mut result = Vec::with_capacity(target_count);

        for sys in selected_systems {
            let games = &by_system[sys];
            let fresh: Vec<&GameRef> = games
                .iter()
                .filter(|g| !recent_set.contains(g.game_id.as_str()))
                .collect();

            let chosen = if !fresh.is_empty() {
                // If multiple fresh games exist, choose pseudo-randomly using uuid entropy
                let idx = (uuid::Uuid::new_v4().as_u128() as usize) % fresh.len();
                fresh[idx]
            } else {
                // Fall back to any enabled game in this system
                let idx = (uuid::Uuid::new_v4().as_u128() as usize) % games.len();
                &games[idx]
            };
            result.push(chosen.clone());
        }

        result
    }
}

/// Async helper: hash ROMs under `dir` in `spawn_blocking`, merge into `state.catalogue`,
/// and atomically persist to `BOSS_CATALOGUE_FILE` (best-effort if default path is unwritable).
pub async fn scan_and_merge_into_state(
    state: &crate::state::SharedState,
    dir: &Path,
) -> io::Result<(usize, usize)> {
    let dir_buf = dir.to_path_buf();
    let scanned = tokio::task::spawn_blocking(move || collect_roms_from_dir(&dir_buf))
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("scan task failed: {e}")))??;

    let (imported, total, snapshot) = {
        let mut cat = state.catalogue.write().await;
        let imported = cat.merge_scanned_games(scanned);
        let total = cat.len();
        (imported, total, cat.clone())
    };

    if let Some(persist_path) = active_catalogue_file_path() {
        let persist_res = tokio::task::spawn_blocking(move || snapshot.save_to_file(&persist_path)).await;
        match persist_res {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "could not persist scanned catalogue");
            }
            Err(e) => {
                tracing::warn!(error = %e, "catalogue persistence task panicked");
            }
        }
    }

    Ok((imported, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_catalog_initialization() {
        let cat = GameCatalogue::new();
        let enabled = cat.list_enabled();
        assert_eq!(enabled.len(), 7);

        assert!(cat.get("dkc").is_some());
        assert!(cat.get("dkc3").is_some());
        assert!(cat.get("batman_returns").is_some());
        assert!(cat.get("bubsy2").is_some());
        assert!(cat.get("sfiii3").is_some());
        assert!(cat.get("mslugx").is_some());
        assert!(cat.get("mvc2").is_some());
    }

    #[test]
    fn test_pick_attract_games_distinct_systems() {
        let cat = GameCatalogue::new();
        let picked = cat.pick_attract_games(&[]);
        assert_eq!(picked.len(), 3);

        let systems: HashSet<String> = picked.iter().map(|g| g.system.clone()).collect();
        assert_eq!(systems.len(), 3, "every picked game must be from a distinct system");
        assert!(systems.contains("snes"));
        assert!(systems.contains("arcade"));
        assert!(systems.contains("naomi"));
    }

    #[test]
    fn test_pick_attract_games_avoids_recent() {
        let cat = GameCatalogue::new();
        // Suppose previous playlist was dkc, sfiii3, mvc2
        let recent = vec!["dkc".into(), "sfiii3".into(), "mvc2".into()];

        for _ in 0..10 {
            let picked = cat.pick_attract_games(&recent);
            assert_eq!(picked.len(), 3);

            let snes_game = picked.iter().find(|g| g.system == "snes").unwrap();
            assert_ne!(snes_game.game_id, "dkc", "should avoid recent dkc");

            let arcade_game = picked.iter().find(|g| g.system == "arcade").unwrap();
            assert_eq!(arcade_game.game_id, "mslugx", "should pick fresh mslugx over sfiii3");

            let naomi_game = picked.iter().find(|g| g.system == "naomi").unwrap();
            assert_eq!(naomi_game.game_id, "mvc2", "falls back to mvc2 as it's the only naomi game");
        }
    }

    #[test]
    fn test_pick_attract_games_respects_disabled() {
        let mut cat = GameCatalogue::new();
        // Disable both arcade games
        cat.set_enabled("sfiii3", false);
        cat.set_enabled("mslugx", false);

        let picked = cat.pick_attract_games(&[]);
        assert_eq!(picked.len(), 2, "only 2 systems remain enabled");

        let systems: HashSet<String> = picked.iter().map(|g| g.system.clone()).collect();
        assert_eq!(systems.len(), 2);
        assert!(systems.contains("snes"));
        assert!(systems.contains("naomi"));
        assert!(!systems.contains("arcade"));
    }

    #[test]
    fn test_resolve_rom_path_finds_astro_roms() {
        let cat = GameCatalogue::new();
        let resolved = cat.resolve_rom_path("Batman Returns.zip");
        assert!(resolved.is_some(), "Batman Returns.zip should resolve in astro/roms or ../astro/roms");
        let path = resolved.unwrap();
        assert!(path.is_file());
    }

    #[test]
    fn test_compute_sha256() {
        let cat = GameCatalogue::new();
        let resolved = cat.resolve_rom_path("Batman Returns.zip").unwrap();
        let hash = compute_sha256(&resolved).unwrap();
        assert_eq!(hash, "c49a290ebf4b60730997a3ffd1347a06cf982479dbb529d3a1d02c3528a3c717");
    }

    #[test]
    fn test_scan_roms_dir_and_atomic_persistence_preserves_enabled() {
        let tmp_root = std::env::temp_dir().join(format!("boss-cat-test-{}", uuid::Uuid::new_v4()));
        let snes_dir = tmp_root.join("roms").join("snes");
        let arcade_dir = tmp_root.join("roms").join("arcade");
        std::fs::create_dir_all(&snes_dir).unwrap();
        std::fs::create_dir_all(&arcade_dir).unwrap();

        let rom_bytes = b"BEERCADE_TEST_ROM_PAYLOAD_V1";
        let expected_hash = beercade_shared::crypto::compute_sha256(rom_bytes);
        std::fs::write(snes_dir.join("Chrono Trigger.sfc"), rom_bytes).unwrap();
        std::fs::write(arcade_dir.join("sfiii3.zip"), b"REAL_SFIII3_ROM").unwrap();
        std::fs::write(arcade_dir.join("readme.txt"), b"ignore").unwrap();

        let mut cat = GameCatalogue::new();
        cat.set_enabled("sfiii3", false);

        let scanned_count = cat.scan_roms_dir(&tmp_root.join("roms")).unwrap();
        assert_eq!(scanned_count, 2);

        let chrono = cat.get("chrono_trigger").unwrap();
        assert_eq!(chrono.game.system, "snes");
        assert_eq!(chrono.game.core, "snes9x");
        assert_eq!(chrono.game.sha256, expected_hash);
        assert!(chrono.enabled);

        // Disabled flag preserved while hash updated from placeholder
        let sf3 = cat.get("sfiii3").unwrap();
        assert!(!sf3.enabled);
        assert_eq!(
            sf3.game.sha256,
            beercade_shared::crypto::compute_sha256(b"REAL_SFIII3_ROM")
        );

        let cat_file = tmp_root.join("catalogue.json");
        cat.save_to_file(&cat_file).unwrap();
        let reloaded = GameCatalogue::load_or_seed(&cat_file);
        assert_eq!(reloaded.get("chrono_trigger").unwrap().game.sha256, expected_hash);
        assert!(!reloaded.get("sfiii3").unwrap().enabled);

        let _ = std::fs::remove_dir_all(&tmp_root);
    }
}
