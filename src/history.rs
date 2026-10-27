//! History listing and restore operations.

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::config;
use crate::markers;

pub struct Version {
    pub path: PathBuf,
    pub timestamp: String,
    pub display_time: String,
    pub size: u64,
}

/// Find the project root for a given file.
/// Uses the same logic as the daemon: walk up looking for project markers,
/// but fall back to the nearest WATCH ROOT (from config), not the file's parent.
/// This ensures history commands find the same path the daemon used.
pub fn find_project_root(file_path: &Path) -> PathBuf {
    let abs = fs::canonicalize(file_path)
        .unwrap_or_else(|_| file_path.to_path_buf());

    let start = if abs.is_dir() {
        abs.clone()
    } else {
        abs.parent().unwrap_or(&abs).to_path_buf()
    };

    let cfg = config::load_or_discover();

    // Find which watch root this file belongs to
    let watch_root = cfg.roots.iter()
        .find(|r| abs.starts_with(r))
        .cloned();

    let boundary = match &watch_root {
        Some(r) => r.clone(),
        None => start.clone(),
    };

    // Walk up from file toward boundary, looking for project markers
    let mut current = start;
    while current.starts_with(&boundary) {
        if markers::has_marker(&current) {
            return current;
        }
        match current.parent() {
            Some(p) if p != current => current = p.to_path_buf(),
            _ => break,
        }
    }

    // No marker found — fall back to watch root (same as daemon does)
    boundary
}

/// List all history versions of a file, newest first.
/// Works for deleted files too: a path that can't be canonicalized is
/// made absolute instead, and its history is still in the store.
pub fn list_versions(file_path: &Path) -> Vec<Version> {
    let abs = fs::canonicalize(file_path)
        .or_else(|_| std::path::absolute(file_path))
        .unwrap_or_else(|_| file_path.to_path_buf());
    versions_in_store(&config::load_or_discover().history_path, &abs)
}

/// Versions of `file` (absolute; need not exist), newest first.
///
/// Rather than re-deriving the project root from markers, this checks the
/// store for each ancestor the daemon could have used as the root. The
/// store records the choice the daemon actually made, so lookups survive
/// marker changes (a later `git init`, a deleted package.json) and never
/// read the project folder, which the sandboxed Finder extension can't.
pub fn versions_in_store(history_path: &Path, file: &Path) -> Vec<Version> {
    let Some(parent) = file.parent() else {
        return vec![];
    };
    for root in candidate_roots(parent) {
        let Ok(rel_dir) = parent.strip_prefix(root) else {
            continue;
        };
        let dir = config::history_dir_for_project(history_path, root).join(rel_dir);
        let found = scan(&dir, file);
        if !found.is_empty() {
            return found;
        }
    }
    vec![]
}

/// The history directory mirroring folder `dir`, if any file in it was
/// ever versioned. It also holds versions of files since deleted.
pub fn folder_history(history_path: &Path, dir: &Path) -> Option<PathBuf> {
    candidate_roots(dir).find_map(|root| {
        let rel = dir.strip_prefix(root).ok()?;
        let mirrored = config::history_dir_for_project(history_path, root).join(rel);
        mirrored.is_dir().then_some(mirrored)
    })
}

/// `dir` and its ancestors, nearest first, down to two levels deep
/// (`/Users/me`). Nothing shallower is a project root, and
/// `history_dir_for_project` can't name `/` or `/Users` anyway.
fn candidate_roots(dir: &Path) -> impl Iterator<Item = &Path> {
    dir.ancestors().filter(|a| {
        a.components()
            .filter(|c| matches!(c, Component::Normal(_)))
            .count()
            >= 2
    })
}

/// Versions of `file` among the entries of `dir`, newest first.
/// Versions are named `<stem>_<YYYYMMDDHHMMSS><.ext>`.
fn scan(dir: &Path, file: &Path) -> Vec<Version> {
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let ext = file
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e))
        .unwrap_or_default();
    let pattern = format!("{}_", stem);

    let mut versions: Vec<Version> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            let ts = name.strip_prefix(&pattern)?.strip_suffix(ext.as_str())?;
            if ts.len() != 14 || !ts.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }

            let display = format!(
                "{}-{}-{} {}:{}:{}",
                &ts[0..4], &ts[4..6], &ts[6..8],
                &ts[8..10], &ts[10..12], &ts[12..14],
            );
            let size = entry.metadata().ok()?.len();

            Some(Version {
                path: entry.path(),
                timestamp: ts.to_string(),
                display_time: display,
                size,
            })
        })
        .collect();

    versions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    versions
}

/// Restore a version by copying it over the current file.
pub fn restore_version(version_path: &Path, original_path: &Path) -> Result<(), String> {
    if !version_path.exists() {
        return Err("Version file not found.".into());
    }
    fs::copy(version_path, original_path)
        .map_err(|e| format!("Failed to restore: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_project() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("myapp");
        fs::create_dir_all(project.join("src")).unwrap();
        fs::create_dir(project.join(".git")).unwrap();
        (tmp, project)
    }

    #[test]
    fn find_project_root_works() {
        let (_tmp, project) = setup_project();
        let file = project.join("src").join("app.js");
        fs::write(&file, "code").unwrap();
        let root = find_project_root(&file);
        // canonicalize both to handle macOS /var → /private/var
        let root_real = fs::canonicalize(&root).unwrap_or(root);
        let file_real = fs::canonicalize(&file).unwrap_or(file);
        assert!(file_real.starts_with(&root_real));
    }

    #[test]
    fn restore_overwrites_current() {
        let (_tmp, project) = setup_project();
        let file = project.join("app.txt");
        fs::write(&file, "current content").unwrap();

        let version = project.join("old_version.txt");
        fs::write(&version, "old content").unwrap();

        restore_version(&version, &file).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "old content");
    }

    #[test]
    fn restore_nonexistent_version_errors() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("app.txt");
        let fake = tmp.path().join("nonexistent.txt");
        let result = restore_version(&fake, &file);
        assert!(result.is_err());
    }

    /// A store holding versions the way the daemon writes them.
    fn store_with(history: &Path, root: &Path, rel_file: &str, stamps: &[&str]) {
        let rel = Path::new(rel_file);
        let dir = config::history_dir_for_project(history, root).join(rel.parent().unwrap());
        fs::create_dir_all(&dir).unwrap();
        let stem = rel.file_stem().unwrap().to_str().unwrap();
        let ext = rel.extension().map(|e| format!(".{}", e.to_str().unwrap())).unwrap_or_default();
        for ts in stamps {
            fs::write(dir.join(format!("{stem}_{ts}{ext}")), ts).unwrap();
        }
    }

    #[test]
    fn finds_versions_without_reading_the_project_folder() {
        // The project folder doesn't exist at all: deleted files, or a
        // sandboxed caller, must still find history.
        let history = TempDir::new().unwrap();
        let root = Path::new("/Users/me/Codebase/app");
        store_with(history.path(), root, "src/main.rs", &["20261027090000", "20261027120000"]);

        let versions = versions_in_store(history.path(), &root.join("src/main.rs"));

        let stamps: Vec<_> = versions.iter().map(|v| v.timestamp.as_str()).collect();
        assert_eq!(stamps, ["20261027120000", "20261027090000"]);
    }

    #[test]
    fn ignores_other_files_with_a_shared_prefix() {
        let history = TempDir::new().unwrap();
        let root = Path::new("/Users/me/Codebase/app");
        store_with(history.path(), root, "app.js", &["20261027090000"]);
        store_with(history.path(), root, "app_v2.js", &["20261027090000"]);
        store_with(history.path(), root, "app.test.js", &["20261027090000"]);

        assert_eq!(versions_in_store(history.path(), &root.join("app.js")).len(), 1);
    }

    #[test]
    fn finds_versions_saved_under_an_outer_root() {
        // Saved while only the outer folder was a project; `inner` became
        // its own project later (git init). The versions must not vanish.
        let history = TempDir::new().unwrap();
        let outer = Path::new("/Users/me/Codebase");
        let inner = outer.join("inner");
        store_with(history.path(), outer, "inner/notes.md", &["20261027090000"]);
        store_with(history.path(), &inner, "other.md", &["20261027100000"]);

        assert_eq!(versions_in_store(history.path(), &inner.join("notes.md")).len(), 1);
    }

    #[test]
    fn folder_history_finds_mirrored_directory() {
        let history = TempDir::new().unwrap();
        let root = Path::new("/Users/me/Codebase/app");
        store_with(history.path(), root, "src/deleted.rs", &["20261027090000"]);

        let dir = folder_history(history.path(), &root.join("src")).unwrap();
        assert!(dir.join("deleted_20261027090000.rs").exists());
        assert!(folder_history(history.path(), &root.join("docs")).is_none());
    }
}
