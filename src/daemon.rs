//! macOS launchd daemon management.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::config;

const LABEL: &str = "com.temprix.daemon";

fn plist_path() -> PathBuf {
    let home = config::dirs_home();
    home.join("Library")
        .join("LaunchAgents")
        .join(format!("{}.plist", LABEL))
}

fn generate_plist(bin_path: &str) -> String {
    let log = config::log_path();
    let log_str = log.to_str().unwrap_or("/tmp/temprix.log");
    let home = config::dirs_home();
    let home_str = home.to_str().unwrap_or("/tmp");

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{bin_path}</string>
    <string>daemon</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{log_str}</string>
  <key>StandardErrorPath</key>
  <string>{log_str}</string>
  <key>WorkingDirectory</key>
  <string>{home_str}</string>
</dict>
</plist>"#
    )
}

pub fn install() -> Result<(), String> {
    let plist = plist_path();
    let plist_dir = plist.parent().unwrap();
    fs::create_dir_all(plist_dir).map_err(|e| e.to_string())?;

    let log_dir = config::temprix_dir().join("logs");
    fs::create_dir_all(&log_dir).map_err(|e| e.to_string())?;

    // Unload existing
    if plist.exists() {
        let _ = Command::new("launchctl")
            .args(["unload", plist.to_str().unwrap()])
            .output();
    }

    // Find our own binary path
    let bin = std::env::current_exe()
        .map_err(|e| e.to_string())?;
    let bin_str = bin.to_str().ok_or("invalid binary path")?;

    // Write plist
    let content = generate_plist(bin_str);
    fs::write(&plist, content).map_err(|e| e.to_string())?;

    // Load
    Command::new("launchctl")
        .args(["load", plist.to_str().unwrap()])
        .output()
        .map_err(|e| e.to_string())?;

    Ok(())
}

pub fn uninstall() -> Result<(), String> {
    let plist = plist_path();
    if !plist.exists() {
        return Err("Not installed.".into());
    }

    let _ = Command::new("launchctl")
        .args(["unload", plist.to_str().unwrap()])
        .output();

    fs::remove_file(&plist).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn is_running() -> bool {
    Command::new("launchctl")
        .args(["list", LABEL])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn log_path() -> PathBuf {
    config::log_path()
}
