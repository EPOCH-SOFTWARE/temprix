//! Path exclusion filter.
//!
//! Single function, no state, no allocations.

use std::path::Path;

const DIRS: &[&str] = &[
    ".history", ".git", "node_modules", ".vscode", ".idea",
    "typings", "out", ".next", ".nuxt", "dist", "build",
    ".turbo", ".svn", ".hg", "venv", ".venv", "__pycache__",
    "target", ".gradle", ".cache", ".parcel-cache",
    "coverage", ".nyc_output", ".pytest_cache",
];

const FILES: &[&str] = &[
    ".DS_Store", "package-lock.json", "yarn.lock", "pnpm-lock.yaml",
    "Thumbs.db", "desktop.ini",
];

/// Extensions WITHOUT the dot — compared directly against OsStr.
const EXTS: &[&str] = &[
    "pyc", "pyo", "o", "a", "dylib", "so", "class",
    "jar", "war", "dll", "exe", "obj",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_git_directory() {
        assert!(is_excluded(Path::new(".git/config")));
        assert!(is_excluded(Path::new("src/.git/objects/abc")));
    }

    #[test]
    fn excludes_node_modules() {
        assert!(is_excluded(Path::new("node_modules/express/index.js")));
        assert!(is_excluded(Path::new("packages/core/node_modules/lodash/fp.js")));
    }

    #[test]
    fn excludes_history_dir() {
        assert!(is_excluded(Path::new(".history/src/app_20260322.js")));
    }

    #[test]
    fn excludes_known_files() {
        assert!(is_excluded(Path::new(".DS_Store")));
        assert!(is_excluded(Path::new("package-lock.json")));
        assert!(is_excluded(Path::new("yarn.lock")));
    }

    #[test]
    fn excludes_binary_extensions() {
        assert!(is_excluded(Path::new("main.o")));
        assert!(is_excluded(Path::new("lib.dylib")));
        assert!(is_excluded(Path::new("module.pyc")));
        assert!(is_excluded(Path::new("App.class")));
    }

    #[test]
    fn allows_source_files() {
        assert!(!is_excluded(Path::new("src/app.js")));
        assert!(!is_excluded(Path::new("main.rs")));
        assert!(!is_excluded(Path::new("utils/helper.py")));
        assert!(!is_excluded(Path::new("deep/nested/file.tsx")));
    }

    #[test]
    fn allows_config_files() {
        assert!(!is_excluded(Path::new("package.json")));
        assert!(!is_excluded(Path::new("tsconfig.json")));
        assert!(!is_excluded(Path::new(".env")));
        assert!(!is_excluded(Path::new("Cargo.toml")));
    }

    #[test]
    fn excludes_build_dirs() {
        assert!(is_excluded(Path::new("dist/bundle.js")));
        assert!(is_excluded(Path::new("build/output.css")));
        assert!(is_excluded(Path::new("target/debug/binary")));
        assert!(is_excluded(Path::new("__pycache__/mod.cpython.pyc")));
    }
}

pub fn is_excluded(rel_path: &Path) -> bool {
    for component in rel_path.components() {
        let s = component.as_os_str().to_str().unwrap_or("");
        if DIRS.contains(&s) {
            return true;
        }
    }

    if let Some(name) = rel_path.file_name().and_then(|n| n.to_str()) {
        if FILES.contains(&name) {
            return true;
        }
        // Compare extension directly — no format!/allocation
        if let Some(ext) = rel_path.extension().and_then(|e| e.to_str()) {
            if EXTS.contains(&ext) {
                return true;
            }
        }
    }

    false
}
