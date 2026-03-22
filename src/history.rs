//! History listing and restore operations.

use std::fs;
use std::path::{Path, PathBuf};

use crate::config;
use crate::markers;

pub struct Version {
    pub path: PathBuf,
    pub timestamp: String,
    pub display_time: String,
    pub size: u64,
}

/// Find the project root for a given file by walking up.
pub fn find_project_root(file_path: &Path) -> PathBuf {
    let mut current = if file_path.is_dir() {
        file_path.to_path_buf()
    } else {
        file_path.parent().unwrap_or(file_path).to_path_buf()
    };

    loop {
        if markers::has_marker(&current) {
            return current;
        }
        match current.parent() {
            Some(p) if p != current => current = p.to_path_buf(),
            _ => return file_path.parent().unwrap_or(file_path).to_path_buf(),
        }
    }
}

/// List all history versions of a file, newest first.
/// Looks in global history storage: ~/.temprix/history/<project>/<rel_path>
pub fn list_versions(file_path: &Path) -> Vec<Version> {
    let abs = match fs::canonicalize(file_path) {
        Ok(p) => p,
        Err(_) => return vec![],
    };

    let project_root = find_project_root(&abs);
    let rel = match abs.strip_prefix(&project_root) {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    let rel_dir = rel.parent().unwrap_or(Path::new(""));
    let stem = abs.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let ext = abs
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e))
        .unwrap_or_default();

    let cfg = config::load_or_discover();
    let project_history = config::history_dir_for_project(&cfg.history_path, &project_root);
    let history_dir = project_history.join(rel_dir);

    if !history_dir.exists() {
        return vec![];
    }

    let pattern = format!("{}_", stem);

    let mut versions: Vec<Version> = fs::read_dir(&history_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            if !name.starts_with(&pattern) || !name.ends_with(&ext) {
                return None;
            }

            let after_stem = &name[pattern.len()..];
            let ts = if ext.is_empty() {
                after_stem.to_string()
            } else {
                after_stem.strip_suffix(&ext)?.to_string()
            };

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
                timestamp: ts,
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
        assert_eq!(find_project_root(&file), project);
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
}
