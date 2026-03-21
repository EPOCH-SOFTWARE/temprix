mod clone;
mod config;
mod daemon;
mod exclude;
mod history;
mod markers;
mod resolve;

use clap::{Parser, Subcommand};
use notify::{Config as NotifyConfig, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use std::{fs, process};

use resolve::Resolver;

// ─── Timestamp ──────────────────────────────────────────────────────

/// VS Code Local History compatible timestamp: YYYYMMDDHHMMSS in local time.
fn timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let time_t = now as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&time_t, &mut tm) };

    format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
    )
}

// ─── Core versioning ────────────────────────────────────────────────

fn version_file(file_path: &Path, project_root: &Path, history_base: &Path) {
    let rel = match file_path.strip_prefix(project_root) {
        Ok(r) => r,
        Err(_) => return,
    };

    let rel_dir = rel.parent().unwrap_or(Path::new(""));
    let stem = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e))
        .unwrap_or_default();

    let dest_name = format!("{}_{}{}", stem, timestamp(), ext);

    // Store in global history: ~/.temprix/history/<project_name>/<relative_path>
    let project_history = config::history_dir_for_project(history_base, project_root);
    let dest = project_history.join(rel_dir).join(dest_name);

    match clone::clone_or_copy(file_path, &dest) {
        Ok(true) => eprintln!("  \x1b[36mcloned\x1b[0m  {}", rel.display()),
        Ok(false) => eprintln!("  \x1b[33mcopied\x1b[0m  {}", rel.display()),
        Err(e) => eprintln!("  \x1b[31merror\x1b[0m   {}: {}", rel.display(), e),
    }
}

// ─── Daemon event loop ──────────────────────────────────────────────

/// Global flag for signal handler. Must be static for the C callback.
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_signal(_sig: libc::c_int) {
    SHUTDOWN.store(true, Ordering::SeqCst);
}

fn run_daemon(roots: Vec<PathBuf>, history_path: PathBuf) {
    // Register signal handlers
    unsafe {
        libc::signal(libc::SIGINT, handle_signal as libc::sighandler_t);
        libc::signal(libc::SIGTERM, handle_signal as libc::sighandler_t);
    }

    let (tx, rx) = mpsc::channel();

    let mut watcher: RecommendedWatcher =
        Watcher::new(tx, NotifyConfig::default()).unwrap_or_else(|e| {
            eprintln!("Failed to create watcher: {}", e);
            process::exit(1);
        });

    for root in &roots {
        if !root.exists() {
            eprintln!("  \x1b[33mskip\x1b[0m    {} (not found)", root.display());
            continue;
        }
        match watcher.watch(root, RecursiveMode::Recursive) {
            Ok(()) => eprintln!("  \x1b[32mwatch\x1b[0m   {}", root.display()),
            Err(e) => eprintln!("  \x1b[31merror\x1b[0m   {}: {}", root.display(), e),
        }
    }

    eprintln!();
    eprintln!("  Listening for changes...");
    eprintln!();

    let mut resolver = Resolver::new();
    let mut dirty: HashSet<PathBuf> = HashSet::new();
    let mut last_event = Instant::now();
    let batch_window = Duration::from_millis(100);

    while !SHUTDOWN.load(Ordering::SeqCst) {
        match rx.recv_timeout(batch_window) {
            Ok(Ok(event)) => {
                if matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                    for path in event.paths {
                        if let Some(root) = roots.iter().find(|r| path.starts_with(r)) {
                            if let Ok(rel) = path.strip_prefix(root) {
                                if !exclude::is_excluded(rel) {
                                    dirty.insert(path);
                                }
                            }
                        }
                    }
                    last_event = Instant::now();
                }
            }
            Ok(Err(e)) => eprintln!("  \x1b[31mwatch error\x1b[0m: {}", e),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if !dirty.is_empty() && last_event.elapsed() >= batch_window {
            for file_path in dirty.drain() {
                let meta = match fs::metadata(&file_path) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                if meta.is_dir() || meta.len() == 0 {
                    continue;
                }

                // Safe: file already passed starts_with check above
                let root = match roots.iter().find(|r| file_path.starts_with(r)) {
                    Some(r) => r,
                    None => continue,
                };
                let project_root = resolver.resolve(&file_path, root);
                version_file(&file_path, &project_root, &history_path);
            }
        }
    }

    eprintln!("  \x1b[32mStopped.\x1b[0m");
}


// ─── CLI ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "temprix")]
#[command(about = "Zero-copy file history. Install once, forget forever.")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Install and start (discovers projects automatically)
    Install,
    /// Stop and remove
    Uninstall,
    /// Show status
    Status,
    /// Show version history for a file
    History {
        /// File to show history for
        file: PathBuf,
    },
    /// Restore a file to a previous version
    Restore {
        /// File to restore
        file: PathBuf,
        /// Version number (from `temprix history`), default: latest
        #[arg(short, long)]
        version: Option<usize>,
    },
    /// Manage watched root directories
    Roots {
        #[command(subcommand)]
        action: RootsAction,
    },
    /// Show recent logs
    Logs {
        #[arg(short, default_value = "50")]
        n: usize,
    },
    /// Re-scan for projects
    Scan,
    /// Run the daemon (called by launchd)
    #[command(hide = true)]
    Daemon,
}

#[derive(Subcommand)]
enum RootsAction {
    /// List all watched roots
    List,
    /// Add a root directory
    Add { path: PathBuf },
    /// Remove a root directory
    Remove { path: PathBuf },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Install => {
            eprintln!();
            eprintln!("  \x1b[1mTEMPRIX\x1b[0m — zero-copy file history");
            eprintln!();
            eprintln!("  Scanning for projects...");

            let cfg = config::discover_and_save();

            if cfg.roots.is_empty() {
                eprintln!("  No projects found.");
                eprintln!("  Run: temprix roots add ~/your-code-folder");
                eprintln!();
                return;
            }

            eprintln!(
                "  \x1b[32mFound {} root(s), {} projects:\x1b[0m",
                cfg.roots.len(),
                cfg.discovered_projects
            );
            for r in &cfg.roots {
                eprintln!("    {}", r.display());
            }
            eprintln!();

            match daemon::install() {
                Ok(()) => {
                    eprintln!("  \x1b[32m✓\x1b[0m Daemon installed and running.");
                    eprintln!("  \x1b[2mLogs: {}\x1b[0m", daemon::log_path().display());
                    eprintln!();
                    eprintln!("  \x1b[2mEvery file change is now versioned instantly.\x1b[0m");
                    eprintln!("  \x1b[2mZero-copy APFS clones. Zero disk overhead.\x1b[0m");
                }
                Err(e) => eprintln!("  \x1b[31m✗\x1b[0m {}", e),
            }
            eprintln!();
        }

        Commands::Uninstall => {
            match daemon::uninstall() {
                Ok(()) => eprintln!("  \x1b[32m✓\x1b[0m Uninstalled."),
                Err(e) => eprintln!("  \x1b[33m!\x1b[0m {}", e),
            }
        }

        Commands::Status => {
            let cfg = config::load_or_discover();
            let running = daemon::is_running();

            eprintln!();
            eprintln!("  \x1b[1mTEMPRIX\x1b[0m");
            eprintln!();
            if running {
                eprintln!("  Status: \x1b[32m● running\x1b[0m");
            } else {
                eprintln!("  Status: \x1b[31m● stopped\x1b[0m");
            }
            eprintln!();
            for r in &cfg.roots {
                let mark = if r.exists() { "\x1b[32m✓\x1b[0m" } else { "\x1b[31m✗\x1b[0m" };
                eprintln!("  {} {}", mark, r.display());
            }
            eprintln!();
        }

        Commands::History { file } => {
            let versions = history::list_versions(&file);

            if versions.is_empty() {
                eprintln!("  No history found for: {}", file.display());
                return;
            }

            eprintln!();
            eprintln!(
                "  \x1b[1m{}\x1b[0m — {} version(s)",
                file.display(),
                versions.len()
            );
            eprintln!();

            for (i, v) in versions.iter().enumerate() {
                let size = if v.size < 1024 {
                    format!("{} B", v.size)
                } else if v.size < 1024 * 1024 {
                    format!("{:.1} KB", v.size as f64 / 1024.0)
                } else {
                    format!("{:.1} MB", v.size as f64 / 1024.0 / 1024.0)
                };

                let label = if i == 0 { " (latest)" } else { "" };
                eprintln!(
                    "  \x1b[2m{:>3}\x1b[0m  {}  \x1b[2m{}\x1b[0m{}",
                    i + 1,
                    v.display_time,
                    size,
                    label,
                );
            }
            eprintln!();
            eprintln!("  \x1b[2mRestore: temprix restore {} --version N\x1b[0m", file.display());
            eprintln!();
        }

        Commands::Restore { file, version } => {
            let versions = history::list_versions(&file);

            if versions.is_empty() {
                eprintln!("  No history found for: {}", file.display());
                return;
            }

            let idx = version.unwrap_or(1);
            if idx == 0 || idx > versions.len() {
                eprintln!("  Invalid version. Use 1-{}.", versions.len());
                return;
            }

            let v = &versions[idx - 1];
            let abs = fs::canonicalize(&file).unwrap_or_else(|_| file.clone());

            // Clone current state before restoring (so restore itself is recoverable)
            let cfg = config::load_or_discover();
            let project_root = history::find_project_root(&abs);
            version_file(&abs, &project_root, &cfg.history_path);
            eprintln!("  \x1b[2mSaved current state before restoring\x1b[0m");

            match history::restore_version(&v.path, &abs) {
                Ok(()) => {
                    eprintln!(
                        "  \x1b[32m✓\x1b[0m Restored to version {} ({})",
                        idx, v.display_time
                    );
                }
                Err(e) => eprintln!("  \x1b[31m✗\x1b[0m {}", e),
            }
        }

        Commands::Roots { action } => match action {
            RootsAction::List => {
                let cfg = config::load_or_discover();
                eprintln!();
                if cfg.roots.is_empty() {
                    eprintln!("  \x1b[2mNo roots. Run: temprix roots add <path>\x1b[0m");
                } else {
                    for r in &cfg.roots {
                        let mark = if r.exists() { "\x1b[32m✓\x1b[0m" } else { "\x1b[31m✗\x1b[0m" };
                        eprintln!("  {} {}", mark, r.display());
                    }
                }
                eprintln!();
            }
            RootsAction::Add { path } => {
                match config::add_root(&path) {
                    Ok(()) => {
                        eprintln!("  \x1b[32m✓\x1b[0m Added: {}", path.display());
                        if daemon::is_running() {
                            eprintln!("  \x1b[2mReinstall to apply: temprix install\x1b[0m");
                        }
                    }
                    Err(e) => eprintln!("  \x1b[33m!\x1b[0m {}", e),
                }
            }
            RootsAction::Remove { path } => {
                match config::remove_root(&path) {
                    Ok(()) => {
                        eprintln!("  \x1b[32m✓\x1b[0m Removed: {}", path.display());
                        if daemon::is_running() {
                            eprintln!("  \x1b[2mReinstall to apply: temprix install\x1b[0m");
                        }
                    }
                    Err(e) => eprintln!("  \x1b[33m!\x1b[0m {}", e),
                }
            }
        },

        Commands::Logs { n } => {
            let log = daemon::log_path();
            if !log.exists() {
                eprintln!("  No logs found. Run 'temprix install' first.");
                return;
            }
            if let Ok(content) = fs::read_to_string(&log) {
                let lines: Vec<&str> = content.lines().collect();
                let start = lines.len().saturating_sub(n);
                for line in &lines[start..] {
                    println!("{}", line);
                }
            }
        }

        Commands::Scan => {
            let cfg = config::discover_and_save();
            eprintln!(
                "  Found {} root(s), {} projects",
                cfg.roots.len(),
                cfg.discovered_projects
            );
            for r in &cfg.roots {
                eprintln!("    {}", r.display());
            }
        }

        Commands::Daemon => {
            let cfg = config::load_or_discover();

            eprintln!();
            eprintln!(
                "  \x1b[1mTEMPRIX\x1b[0m daemon (pid {})",
                process::id()
            );
            eprintln!("  \x1b[2mAPFS clonefile — zero-copy versioning\x1b[0m");
            eprintln!();

            if cfg.roots.is_empty() {
                eprintln!("  No roots configured.");
                loop {
                    std::thread::sleep(Duration::from_secs(3600));
                }
            }

            eprintln!("  \x1b[2mHistory: {}\x1b[0m", cfg.history_path.display());
            eprintln!();

            run_daemon(cfg.roots, cfg.history_path);
        }
    }
}
