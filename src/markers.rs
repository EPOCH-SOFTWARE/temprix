//! Single source of truth for project markers.

use std::path::Path;

pub const MARKERS: &[&str] = &[
    ".git",
    "package.json",
    "Cargo.toml",
    "go.mod",
    "pyproject.toml",
    "pom.xml",
    "build.gradle",
    "pubspec.yaml",
    "Gemfile",
    "mix.exs",
    "CMakeLists.txt",
    "Makefile",
    ".xcodeproj",
    "dune-project",
];

pub fn has_marker(dir: &Path) -> bool {
    MARKERS.iter().any(|m| dir.join(m).exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn detects_git() {
        let tmp = TempDir::new().unwrap();
        assert!(!has_marker(tmp.path()));

        fs::create_dir(tmp.path().join(".git")).unwrap();
        assert!(has_marker(tmp.path()));
    }

    #[test]
    fn detects_package_json() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("package.json"), "{}").unwrap();
        assert!(has_marker(tmp.path()));
    }

    #[test]
    fn detects_cargo_toml() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "[package]").unwrap();
        assert!(has_marker(tmp.path()));
    }

    #[test]
    fn empty_dir_has_no_marker() {
        let tmp = TempDir::new().unwrap();
        assert!(!has_marker(tmp.path()));
    }
}
