//! The history views opened from Finder's "Show TEMPRIX History" menu item
//! (macos/FinderExtension) and by `temprix reveal`.
//!
//! The history store keeps every file's versions of a directory in one
//! folder under compact names (`app_20260322012530.js`). That layout suits
//! the daemon, not a person hunting for one file's past. Rather than
//! reshape the store, `reveal` builds a folder holding only the chosen
//! file's versions under readable names. Entries are APFS clones: the view
//! costs no disk space, and deleting it never touches real history.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{clone, config, history};

fn views_dir() -> PathBuf {
    config::temprix_dir().join("views")
}

// ─── Reveal ─────────────────────────────────────────────────────────

/// Name of the shared view for a multi-item selection. Fixed, so each
/// multi-item reveal replaces the last instead of piling up.
const SELECTION_VIEW: &str = "Selected items (history)";

/// A selected item that has history.
enum Found {
    File { name: String, versions: Vec<history::Version> },
    Folder { name: String, dir: PathBuf },
}

/// Open the history of `targets` in one Finder window and return the
/// names of items that had none.
///
/// - One file: a view of its versions, newest selected, so Space (Quick
///   Look) and the arrow keys step through them.
/// - One folder: its mirrored history directory, which also holds versions
///   of files since deleted from it.
/// - Several items: one folder with a subfolder per file and a link per
///   folder. Ten selected files open one window, not ten.
///
/// Errors only if nothing selected has history.
pub fn reveal(targets: &[PathBuf]) -> Result<Vec<String>, String> {
    let history_path = config::load_or_discover().history_path;
    let mut found = Vec::new();
    let mut missing = Vec::new();

    for target in targets {
        // Not canonicalize-only: a deleted file has no real path but
        // still has history.
        let abs = fs::canonicalize(target)
            .or_else(|_| std::path::absolute(target))
            .map_err(|e| format!("{}: {}", target.display(), e))?;
        let name = display_name(&abs);
        let item = if abs.is_dir() {
            history::folder_history(&history_path, &abs).map(|dir| Found::Folder { name: name.clone(), dir })
        } else {
            let versions = history::versions_in_store(&history_path, &abs);
            (!versions.is_empty()).then(|| Found::File { name: name.clone(), versions })
        };
        match item {
            Some(f) => found.push(f),
            None => missing.push(name),
        }
    }

    let failed = |e: io::Error| format!("could not build history view: {e}");
    match found.as_slice() {
        [] => return Err(format!("No history for {}", missing.join(", "))),
        [Found::File { name, versions }] => {
            let newest = build_view(&views_dir(), name, versions).map_err(failed)?;
            open(&["-R"], &newest)?;
        }
        // Paths are absolute here, so they can never be read as flags.
        [Found::Folder { dir, .. }] => open(&[], dir)?,
        _ => {
            let view = build_selection_view(&views_dir().join(SELECTION_VIEW), &found).map_err(failed)?;
            open(&[], &view)?;
        }
    }
    Ok(missing)
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// One folder for a multi-item selection: `<file> (history)/` with each
/// file's versions, and `<folder> (folder history)` linking to each
/// folder's history. Same-named items (two `index.js`) get " 2", " 3"…
fn build_selection_view(view: &Path, found: &[Found]) -> io::Result<PathBuf> {
    // remove_dir_all removes symlinks without following them, so the
    // folder-history links are dropped and the history they point to stays.
    if view.symlink_metadata().is_ok() {
        fs::remove_dir_all(view)?;
    }
    fs::create_dir_all(view)?;

    let mut taken = HashSet::new();
    let mut unique = |name: &str| {
        let mut label = name.to_string();
        let mut n = 2;
        while !taken.insert(label.clone()) {
            label = format!("{name} {n}");
            n += 1;
        }
        label
    };

    for item in found {
        match item {
            Found::File { name, versions } => {
                let folder = view.join(format!("{} (history)", unique(name)));
                build_view_at(&folder, name, versions)?;
            }
            Found::Folder { name, dir } => {
                let link = view.join(format!("{} (folder history)", unique(name)));
                std::os::unix::fs::symlink(dir, link)?;
            }
        }
    }
    Ok(view.to_path_buf())
}

fn open(flags: &[&str], path: &Path) -> Result<(), String> {
    let status = Command::new("open")
        .args(flags)
        .arg(path)
        .status()
        .map_err(|e| format!("open: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("open failed for {}", path.display()))
    }
}

/// Build `<views>/<name> (history)/` with one clone per version and return
/// the newest clone's path. `versions` must be newest-first, as
/// `history::list_versions` returns them.
///
/// The view for a given name is replaced on each call, so views of other
/// files stay open while this one refreshes, and the folder never holds
/// more than one view per file name.
fn build_view(views: &Path, name: &str, versions: &[history::Version]) -> io::Result<PathBuf> {
    // The suffix keeps Finder from treating the folder as a document
    // package: a folder named "talk.key" would show as a Keynote file.
    let view = views.join(format!("{name} (history)"));
    if view.exists() {
        fs::remove_dir_all(&view)?;
    }
    build_view_at(&view, name, versions)
}

/// Fill a new folder `view` with clones of `versions`; return the newest.
fn build_view_at(view: &Path, name: &str, versions: &[history::Version]) -> io::Result<PathBuf> {
    fs::create_dir_all(view)?;

    let mut newest = None;
    for v in versions {
        let dest = view.join(entry_name(v, name));
        clone::clone_or_copy(&v.path, &dest)?;
        newest.get_or_insert(dest);
    }
    newest.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no versions"))
}

/// "2026-10-27 12.43.01 content.pdf"
/// Date first: names sort chronologically and a dotfile's versions are not
/// hidden. Dots replace colons because Finder displays ':' as '/'.
fn entry_name(v: &history::Version, name: &str) -> String {
    format!("{} {}", v.display_time.replace(':', "."), name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn version(dir: &Path, ts: &str, content: &str) -> history::Version {
        let path = dir.join(format!("content_{ts}.pdf"));
        fs::write(&path, content).unwrap();
        history::Version {
            path,
            timestamp: ts.to_string(),
            display_time: format!(
                "{}-{}-{} {}:{}:{}",
                &ts[0..4], &ts[4..6], &ts[6..8], &ts[8..10], &ts[10..12], &ts[12..14]
            ),
            size: content.len() as u64,
        }
    }

    #[test]
    fn entry_names_are_readable_and_sort_chronologically() {
        let tmp = TempDir::new().unwrap();
        let older = version(tmp.path(), "20261027092301", "a");
        let newer = version(tmp.path(), "20261027124301", "b");
        assert_eq!(entry_name(&newer, "content.pdf"), "2026-10-27 12.43.01 content.pdf");
        assert!(entry_name(&older, "content.pdf") < entry_name(&newer, "content.pdf"));
        // Dotfiles must not produce hidden view entries.
        assert!(!entry_name(&newer, ".env").starts_with('.'));
    }

    #[test]
    fn view_holds_only_this_files_versions_and_returns_newest() {
        let tmp = TempDir::new().unwrap();
        let store = tmp.path().join("store");
        fs::create_dir(&store).unwrap();
        // Newest first, as history::list_versions returns them.
        let versions = vec![
            version(&store, "20261027124301", "v2"),
            version(&store, "20261027092301", "v1"),
        ];
        let views = tmp.path().join("views");

        let newest = build_view(&views, "content.pdf", &versions).unwrap();

        assert_eq!(fs::read_to_string(&newest).unwrap(), "v2");
        let mut names: Vec<_> = fs::read_dir(newest.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["2026-10-27 09.23.01 content.pdf", "2026-10-27 12.43.01 content.pdf"]);
    }

    #[test]
    fn rebuilding_a_view_drops_stale_entries_and_keeps_other_views() {
        let tmp = TempDir::new().unwrap();
        let store = tmp.path().join("store");
        fs::create_dir(&store).unwrap();
        let views = tmp.path().join("views");
        let v1 = version(&store, "20261027092301", "v1");
        let v2 = version(&store, "20261027124301", "v2");

        build_view(&views, "other.txt", std::slice::from_ref(&v1)).unwrap();
        build_view(&views, "content.pdf", std::slice::from_ref(&v1)).unwrap();
        let newest = build_view(&views, "content.pdf", std::slice::from_ref(&v2)).unwrap();

        assert_eq!(fs::read_dir(newest.parent().unwrap()).unwrap().count(), 1);
        assert!(views.join("other.txt (history)").is_dir());
    }

    #[test]
    fn selection_view_groups_items_and_dedupes_names() {
        let tmp = TempDir::new().unwrap();
        let store = tmp.path().join("store");
        let folder_history = tmp.path().join("folder-history");
        fs::create_dir_all(&store).unwrap();
        fs::create_dir_all(&folder_history).unwrap();
        let v = version(&store, "20261027124301", "v");
        let found = vec![
            Found::File { name: "index.js".into(), versions: vec![v] },
            Found::File {
                name: "index.js".into(),
                versions: vec![version(&store, "20261027130000", "w")],
            },
            Found::Folder { name: "src".into(), dir: folder_history.clone() },
        ];

        let view = build_selection_view(&tmp.path().join("sel"), &found).unwrap();

        let mut names: Vec<_> = fs::read_dir(&view)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["index.js (history)", "index.js 2 (history)", "src (folder history)"]);
        assert_eq!(fs::read_link(view.join("src (folder history)")).unwrap(), folder_history);
    }

    #[test]
    fn rebuilding_selection_view_never_deletes_linked_history() {
        // The view links to real history folders. Clearing the old view
        // must remove the link, not what it points to.
        let tmp = TempDir::new().unwrap();
        let real_history = tmp.path().join("history");
        fs::create_dir_all(&real_history).unwrap();
        fs::write(real_history.join("keep_20261027090000.rs"), "precious").unwrap();
        let found = vec![
            Found::Folder { name: "a".into(), dir: real_history.clone() },
            Found::Folder { name: "b".into(), dir: real_history.clone() },
        ];
        let view = tmp.path().join("sel");

        build_selection_view(&view, &found).unwrap();
        build_selection_view(&view, &found).unwrap();

        assert_eq!(fs::read_to_string(real_history.join("keep_20261027090000.rs")).unwrap(), "precious");
    }
}
