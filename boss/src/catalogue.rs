//! Game Catalogue & Attract Mode Scheduler for B.O.S.S.
//!
//! Tracks available titles, hashes, systems, libretro cores, and disk paths.
//! Selects 2-3 games from distinct systems for cabinet attract mode.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use beercade_shared::models::GameRef;

/// Catalogue entry pairing game metadata with its enabled/disabled status.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub game: GameRef,
    pub enabled: bool,
}

/// Compute lower-case hex SHA-256 for a file on disk without loading it into RAM.
pub fn compute_sha256<P: AsRef<Path>>(path: P) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
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

    /// Enable or disable a game. Returns true if the game was found.
    pub fn set_enabled(&mut self, game_id: &str, enabled: bool) -> bool {
        if let Some(entry) = self.entries.get_mut(game_id) {
            entry.enabled = enabled;
            true
        } else {
            false
        }
    }

    /// Import a ROM file from disk by hashing it with SHA-256 and measuring its size.
    pub fn import_rom_file(&mut self, path: &Path, system: &str, core: &str) -> io::Result<GameRef> {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid file name"))?
            .to_string();
        let metadata = std::fs::metadata(path)?;
        let size_bytes = metadata.len();
        let sha256 = compute_sha256(path)?;
        let game_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&file_name)
            .to_lowercase()
            .replace(' ', "_");
        let title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&file_name)
            .to_string();

        let game = GameRef {
            game_id,
            title,
            system: system.to_string(),
            core: core.to_string(),
            file_name,
            size_bytes,
            sha256,
            artwork_url: None,
        };
        self.insert(game.clone(), true);
        Ok(game)
    }

    /// Resolve a ROM file path on disk across standard B.E.E.R.C.A.D.E. storage paths.
    ///
    /// Checks:
    /// - `BOSS_ROMS_DIR` env variable if configured
    /// - `/data/beercade/games/{file_name}`
    /// - `/data/beercade/roms/{file_name}`
    /// - `/data/beercade/roms/snes/{file_name}`
    /// - fallback `astro/roms/{file_name}` and `../astro/roms/{file_name}`
    pub fn resolve_rom_path(&self, file_name: &str) -> Option<PathBuf> {
        let mut candidates = Vec::new();
        if let Ok(env_dir) = std::env::var("BOSS_ROMS_DIR") {
            candidates.push(PathBuf::from(env_dir));
        }
        candidates.push(PathBuf::from("/data/beercade/games"));
        candidates.push(PathBuf::from("/data/beercade/roms"));
        candidates.push(PathBuf::from("/data/beercade/roms/snes"));
        candidates.push(PathBuf::from("astro/roms"));
        candidates.push(PathBuf::from("../astro/roms"));

        for base in candidates {
            let candidate = base.join(file_name);
            if candidate.is_file() {
                return Some(candidate);
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
}
