//! Project root resolution.
//!
//! Given a file path, walks up the directory tree to find the nearest
//! project marker (.git, package.json, etc.). Results are cached
//! permanently — a HashMap lookup per event after the first resolution.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::markers;

pub struct Resolver {
    cache: HashMap<PathBuf, PathBuf>,
}

impl Resolver {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// Resolve the project root for a file path.
    /// Walks up toward `boundary`, looking for project markers.
    /// Caches every directory traversed.
    pub fn resolve(&mut self, file_path: &Path, boundary: &Path) -> PathBuf {
        let dir = match file_path.parent() {
            Some(d) => d,
            None => return boundary.to_path_buf(),
        };

        if let Some(root) = self.cache.get(dir) {
            return root.clone();
        }

        let mut trail = Vec::new();
        let mut current = dir.to_path_buf();

        while current.starts_with(boundary) {
            trail.push(current.clone());

            if markers::has_marker(&current) {
                for d in &trail {
                    self.cache.insert(d.clone(), current.clone());
                }
                return current;
            }

            match current.parent() {
                Some(p) if p != current => current = p.to_path_buf(),
                _ => break,
            }
        }

        for d in &trail {
            self.cache.insert(d.clone(), boundary.to_path_buf());
        }
        boundary.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn finds_project_root_by_git() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("myapp");
        let src = project.join("src").join("components");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir(project.join(".git")).unwrap();

        let file = src.join("App.tsx");
        fs::write(&file, "content").unwrap();

        let mut r = Resolver::new();
        let root = r.resolve(&file, tmp.path());
        assert_eq!(root, project);
    }

    #[test]
    fn caches_resolved_root() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("myapp");
        let src = project.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::create_dir(project.join(".git")).unwrap();

        let file1 = src.join("a.js");
        let file2 = src.join("b.js");
        fs::write(&file1, "a").unwrap();
        fs::write(&file2, "b").unwrap();

        let mut r = Resolver::new();
        let root1 = r.resolve(&file1, tmp.path());
        assert_eq!(root1, project);
        assert!(r.cache.contains_key(&src.to_path_buf()));

        // Second resolve should hit cache
        let root2 = r.resolve(&file2, tmp.path());
        assert_eq!(root2, project);
    }

    #[test]
    fn falls_back_to_boundary() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("random");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("file.txt");
        fs::write(&file, "x").unwrap();

        let mut r = Resolver::new();
        let root = r.resolve(&file, tmp.path());
        assert_eq!(root, tmp.path());
    }

    #[test]
    fn nested_projects_resolve_to_nearest() {
        let tmp = TempDir::new().unwrap();
        let outer = tmp.path().join("mono");
        let inner = outer.join("packages").join("core");
        fs::create_dir_all(&inner).unwrap();
        fs::create_dir(outer.join(".git")).unwrap();
        fs::create_dir(inner.join(".git")).unwrap(); // nested git

        let file = inner.join("src").join("index.ts");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "export default {}").unwrap();

        let mut r = Resolver::new();
        let root = r.resolve(&file, tmp.path());
        // Should resolve to inner, not outer
        assert_eq!(root, inner);
    }
}
