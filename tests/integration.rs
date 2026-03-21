//! Integration tests for TEMPRIX.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

fn temprix_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_temprix"))
}

#[test]
fn scan_discovers_projects() {
    let output = Command::new(temprix_bin())
        .arg("scan")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Found"));
}

#[test]
fn status_reports_state() {
    let output = Command::new(temprix_bin())
        .arg("status")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("TEMPRIX"));
    assert!(stderr.contains("running") || stderr.contains("stopped"));
}

#[test]
fn history_no_versions_for_new_file() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("testproj");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::create_dir(project.join(".git")).unwrap();

    let file = project.join("src").join("new.js");
    fs::write(&file, "brand new").unwrap();

    let output = Command::new(temprix_bin())
        .arg("history")
        .arg(&file)
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("No history found"));
}

#[test]
fn restore_invalid_version_number() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("testproj");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir(project.join(".git")).unwrap();

    let file = project.join("app.txt");
    fs::write(&file, "content").unwrap();

    let output = Command::new(temprix_bin())
        .arg("restore")
        .arg(&file)
        .args(["--version", "99"])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    // Either "No history found" or "Invalid version"
    assert!(stderr.contains("No history") || stderr.contains("Invalid version"));
    assert_eq!(fs::read_to_string(&file).unwrap(), "content");
}

#[test]
fn roots_list_works() {
    let output = Command::new(temprix_bin())
        .args(["roots", "list"])
        .output()
        .unwrap();
    assert!(output.status.success());
}

#[test]
fn help_shows_all_commands() {
    let output = Command::new(temprix_bin())
        .arg("--help")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("install"));
    assert!(stdout.contains("uninstall"));
    assert!(stdout.contains("status"));
    assert!(stdout.contains("history"));
    assert!(stdout.contains("restore"));
    assert!(stdout.contains("roots"));
    assert!(stdout.contains("logs"));
    assert!(stdout.contains("scan"));
}
