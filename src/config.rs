//! Global config at ~/.temprix/config.json
//! and auto-discovery of project root directories.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::markers;

/// Directories in ~/ that are NEVER code — skip during auto-discovery.
/// If a user clones a repo into Downloads or Desktop, auto-discovery
/// will still find it because it checks for project markers (.git, etc.)
/// inside these directories. We only skip dirs that are NEVER project hubs.
const SKIP: &[&str] = &[
    "node_modules", ".git", ".hg", ".svn",
    "Library", "Applications",                // macOS system
    ".Trash", ".cache", ".npm", ".nvm",        // hidden/tooling
    ".local", ".config", ".docker", ".cargo",  // tool config
    "Pictures", "Movies", "Music",             // media
    "Public", "Sites",                         // macOS defaults
    ".zsh_sessions", ".ssh", ".gnupg", ".cups",// system dotfiles
];

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub roots: Vec<PathBuf>,
    #[serde(default)]
    pub discovered_projects: usize,
    /// Where to store version history.
    /// Default: ~/.temprix/history
    /// Versions are stored as: {history_path}/{project_name}/{relative_path}
    #[serde(default = "default_history_path")]
    pub history_path: PathBuf,
}

fn default_history_path() -> PathBuf {
    dirs_home().join(".temprix").join("history")
}

/// Get the history storage directory for a specific project.
/// e.g. /Users/raj/Codebase/myapp → ~/.temprix/history/Codebase__myapp/
///
/// Uses the last two path components to create a readable name,
/// with a short hash suffix to avoid collisions.
pub fn history_dir_for_project(history_path: &Path, project_root: &Path) -> PathBuf {
    // Take last 2 components for readability: "Codebase/TEMPRIX" → "Codebase__TEMPRIX"
    let components: Vec<&str> = project_root
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();

    let name = if components.len() >= 2 {
        format!(
            "{}__{}",
            components[components.len() - 2],
            components[components.len() - 1]
        )
    } else if let Some(last) = components.last() {
        last.to_string()
    } else {
        "unknown".to_string()
    };

    // Short hash of full path to avoid collisions
    let hash = simple_hash(project_root.to_str().unwrap_or(""));

    history_path.join(format!("{}-{:x}", name, hash))
}

/// Simple non-cryptographic hash for path disambiguation.
fn simple_hash(s: &str) -> u32 {
    let mut h: u32 = 5381;
    for b in s.bytes() {
        h = h.wrapping_mul(33).wrapping_add(b as u32);
    }
    h & 0xFFFF // 16-bit — short enough for folder name
}

pub fn temprix_dir() -> PathBuf {
    dirs_home().join(".temprix")
}

pub fn config_path() -> PathBuf {
    temprix_dir().join("config.json")
}

pub fn log_path() -> PathBuf {
    temprix_dir().join("logs").join("daemon.log")
}

pub fn dirs_home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()))
}

pub fn load_or_discover() -> Config {
    let path = config_path();
    if path.exists() {
        if let Ok(data) = fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str::<Config>(&data) {
                return cfg;
            }
        }
    }
    discover_and_save()
}

pub fn discover_and_save() -> Config {
    let home = dirs_home();
    let (roots, projects) = discover_roots(&home);
    let cfg = Config {
        roots,
        discovered_projects: projects,
        history_path: default_history_path(),
    };
    save(&cfg);
    cfg
}

pub fn save(cfg: &Config) {
    let dir = temprix_dir();
    let _ = fs::create_dir_all(&dir);
    let _ = fs::create_dir_all(dir.join("logs"));
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        let _ = fs::write(config_path(), json);
    }
}

pub fn add_root(path: &Path) -> Result<(), String> {
    let abs = fs::canonicalize(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    if !abs.is_dir() {
        return Err(format!("Not a directory: {}", abs.display()));
    }
    let mut cfg = load_or_discover();
    if cfg.roots.contains(&abs) {
        return Err(format!("Already tracking: {}", abs.display()));
    }
    cfg.roots.push(abs);
    save(&cfg);
    Ok(())
}

pub fn remove_root(path: &Path) -> Result<(), String> {
    let abs = fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf());
    let mut cfg = load_or_discover();
    let before = cfg.roots.len();
    cfg.roots.retain(|r| r != &abs);
    if cfg.roots.len() == before {
        return Err(format!("Not tracking: {}", abs.display()));
    }
    save(&cfg);
    Ok(())
}

fn discover_roots(home: &Path) -> (Vec<PathBuf>, usize) {
    let mut roots = Vec::new();
    let mut total = 0;

    let entries = match fs::read_dir(home) {
        Ok(e) => e,
        Err(_) => return (roots, total),
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_str().unwrap_or("");

        if SKIP.contains(&name_str) {
            continue;
        }

        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        if markers::has_marker(&path) {
            roots.push(path);
            total += 1;
            continue;
        }

        let mut count = 0;
        if let Ok(children) = fs::read_dir(&path) {
            for child in children.flatten() {
                let cname = child.file_name();
                if cname.to_str().map_or(true, |s| s.starts_with('.')) {
                    continue;
                }
                let cpath = child.path();
                if cpath.is_dir() && markers::has_marker(&cpath) {
                    count += 1;
                    if count >= 2 {
                        break;
                    }
                }
            }
        }
        if count >= 2 {
            roots.push(path);
            total += count;
        }
    }

    (roots, total)
}
