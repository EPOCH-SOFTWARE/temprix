<p align="center">
  <h1 align="center">⏱️ TEMPRIX</h1>
  <p align="center">
    <strong>Zero-copy file history for macOS</strong><br>
    Versions every file change instantly using APFS <code>clonefile()</code><br>
    No disk overhead · No CPU usage · No configuration
  </p>
  <p align="center">
    <a href="#install">📦 Install</a> · <a href="#commands">⌨️ Commands</a> · <a href="#how-it-works">🔍 How It Works</a> · <a href="LICENSE">📄 License</a>
  </p>
</p>

---

## 🤔 Why?

AI coding tools like **Claude Code**, **Cursor**, and **Windsurf** modify files directly on disk — bypassing VS Code's save events. That means **VS Code's Local History extension never sees the changes**. No history, no undo, no safety net.

TEMPRIX fixes that. It watches your filesystem at the kernel level and creates instant, zero-overhead APFS clones on every file change — from any source.

## 💡 The Insight

Every file history tool copies the entire file on every change. That's expensive, so they add complexity: debouncing, batching, size limits, retention policies, deduplication.

TEMPRIX uses APFS `clonefile()` instead — a copy-on-write clone that shares physical disk blocks with the original. It takes **nanoseconds** and uses **zero additional disk space**. Only when the original is later modified does APFS allocate new blocks — and only for the changed portions.

> When versioning costs nothing, all the complexity disappears.

## ⚡ Performance

Benchmarked on Apple Silicon, 100 iterations per file size:

| File Size | `clonefile` | `copyfile` | Speedup | Disk (100 versions) |
|-----------|-------------|------------|---------|---------------------|
| 1 KB | 0.10 ms | 0.22 ms | **2x** | 0 KB vs 400 KB |
| 100 KB | 0.12 ms | 0.33 ms | **3x** | 0 KB vs 10 MB |
| 1 MB | 0.12 ms | 1.35 ms | **12x** | 0 KB vs 100 MB |
| 10 MB | 0.10 ms | 9.92 ms | **98x** | 0 KB vs 1 GB |
| 50 MB | 0.10 ms | 116.53 ms | **1203x** | 0 KB vs 5 GB |

`clonefile` is **O(1)**. `copyfile` is **O(n)**. The gap only grows.

> 🪶 **Daemon footprint:** 643 KB binary · ~3 MB RAM · 0% CPU at idle

## 📦 Install

```bash
# Build from source
git clone https://github.com/EPOCH-SOFTWARE/temprix.git
cd temprix
cargo build --release

# Copy to PATH
cp target/release/temprix ~/.local/bin/

# Install (auto-discovers projects, starts on login, runs forever)
temprix install
```

That's it. You never touch it again.

## 🔍 How It Works

1. 🔎 **Auto-discovers** your project directories by scanning `~/` for `.git`, `package.json`, `Cargo.toml`, etc.
2. 👁️ **Watches** all discovered roots using macOS FSEvents (kernel-level, zero polling)
3. 📸 **On every file change**, calls `clonefile()` — instant, zero-copy APFS clone
4. 🗂️ **Stores versions** in `~/.temprix/history/` organized by project, mirroring your folder structure
5. 🔄 **Runs as a launchd daemon** — starts on login, restarts on crash, invisible

```
~/.temprix/
├── config.json                         ← roots, settings
├── logs/daemon.log
└── history/
    ├── Codebase__myapp-a3f2/           ← project versions
    │   └── src/
    │       ├── App_20260322012530.tsx
    │       └── App_20260322014500.tsx
    └── Codebase__api-b1c9/
        └── routes/
            └── auth_20260322010000.rs
```

> 🛡️ Delete your project folder — versions survive in `~/.temprix/history/`.

## ⌨️ Commands

```bash
temprix install              # 🚀 Auto-discover projects, install daemon
temprix uninstall            # 🛑 Stop and remove daemon
temprix status               # 📊 Show daemon status and watched roots

temprix history <file>       # 📜 List all versions of a file
temprix restore <file> -v N  # ⏪ Restore to version N (auto-saves current state first)

temprix roots list           # 📂 Show watched root directories
temprix roots add <path>     # ➕ Add a root
temprix roots remove <path>  # ➖ Remove a root

temprix scan                 # 🔎 Re-discover projects
temprix logs                 # 📋 Show daemon logs
```

## 🚫 What Gets Excluded

Everything inside your project roots **except**:

| Category | Excluded |
|----------|----------|
| 📦 Dependencies | `node_modules`, `venv`, `.venv`, `target`, `.gradle` |
| 🏗️ Build output | `dist`, `build`, `out`, `.next`, `.nuxt` |
| 🔀 VCS internals | `.git`, `.svn`, `.hg` |
| 🛠️ IDE config | `.vscode`, `.idea` |
| ⚙️ Compiled binaries | `.o`, `.dylib`, `.class`, `.exe`, `.dll` |
| 🔒 Lockfiles | `package-lock.json`, `yarn.lock`, `pnpm-lock.yaml` |
| 💻 OS files | `.DS_Store`, `Thumbs.db` |

## 📋 Requirements

- 🍎 macOS 10.13+ (APFS required — default since High Sierra 2017)
- 🦀 Rust 1.70+ (to build from source)

## 🆚 How is this different from...

| Tool | Limitation |
|------|-----------|
| **VS Code Local History** | Only tracks editor save events. External tools (Claude Code, Cursor, terminal) bypass it entirely. |
| **Time Machine** | Volume-level snapshots on an hourly schedule. Can't version individual files on every change. |
| **Git** | Requires explicit commits. TEMPRIX versions automatically on every save, from any source. |

## 🏗️ Architecture

```
macOS kernel (FSEvents)
        │
        ▼
  notify crate ← receives file change events
        │
        ▼
  exclude check ← skip node_modules, .git, etc.
        │
        ▼
  project root resolution ← walk up to find .git (cached)
        │
        ▼
  clonefile() ← APFS COW clone: 0.1ms, 0 bytes
        │
        ▼
  ~/.temprix/history/<project>/<path>_<timestamp>.<ext>
```

> 6 source files · 1 dependency (`notify` for FSEvents) · 643 KB binary · No runtime

## 📄 License

MIT
