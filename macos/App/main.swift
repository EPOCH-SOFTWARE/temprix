// TEMPRIX.app host. Two ways it gets opened:
//
// - By the user (double-click): set TEMPRIX up — start the daemon, switch
//   on the Finder menu item — and say so. For someone who downloaded the
//   app, this is the whole install.
// - By a temprix:// URL from the sandboxed Finder extension: run the
//   bundled CLI, which the extension's sandbox does not allow. No UI; quits
//   when idle. When nothing selected has history it plays the alert sound.
//
//   temprix://reveal?path=/a/b.txt&path=/a/c    →    temprix reveal /a/b.txt /a/c

import AppKit
import os

private let log = Logger(subsystem: "com.temprix.app", category: "host")

/// How long to linger after the last request. Keeps a burst of clicks in
/// one process instead of relaunching for each.
private let idleExit: TimeInterval = 2

private let extensionID = "com.temprix.app.finder"
private let pluginkit = URL(fileURLWithPath: "/usr/bin/pluginkit")

final class AppDelegate: NSObject, NSApplicationDelegate {
    private var idleTimer: Timer?

    func applicationDidFinishLaunching(_ notification: Notification) {
        // False when macOS launched the app to open a temprix:// URL.
        let openedByUser = notification.userInfo?[NSApplication.launchIsDefaultUserInfoKey] as? Bool ?? false
        if openedByUser {
            setUp()
        } else {
            scheduleExit()
        }
    }

    /// Double-clicked again while still running from a Finder click.
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        setUp()
        return false
    }

    func application(_ application: NSApplication, open urls: [URL]) {
        for url in urls {
            handle(url)
        }
        scheduleExit()
    }

    // ─── Setup ──────────────────────────────────────────────────────────

    /// What `make install` does for a source build, for a downloaded app.
    private func setUp() {
        idleTimer?.invalidate()
        let bundle = Bundle.main.bundleURL

        // Opened straight from a download, macOS runs the app from a random
        // read-only copy ("App Translocation") that vanishes later. A daemon
        // installed from there would break at the next login.
        if bundle.path.contains("/AppTranslocation/") {
            tell("Move TEMPRIX to Applications",
                 "Drag TEMPRIX from your Downloads folder into Applications, then open it from there.")
            return NSApp.terminate(nil)
        }
        guard let cli = Bundle.main.url(forAuxiliaryExecutable: "temprix") else {
            tell("TEMPRIX is damaged", "Download it again and replace the copy in Applications.")
            return NSApp.terminate(nil)
        }

        // `temprix install` re-discovers projects, which would drop folders
        // added with `temprix roots add`. Only run it when the daemon isn't
        // already running this copy of TEMPRIX (first open, or after moving
        // or updating the app).
        if !daemonRuns(cli) {
            let install = run(cli, ["install"])
            if install.status != 0 {
                tell("TEMPRIX could not start", install.output)
                return NSApp.terminate(nil)
            }
        }

        // Re-register rather than just enable: after an update replaced the
        // app, Finder keeps talking to the old extension until its
        // registration changes. A new registration starts switched off;
        // "use" switches it on, and Finder loads this build.
        let appex = bundle.appendingPathComponent("Contents/PlugIns/TemprixFinder.appex").path
        _ = run(pluginkit, ["-r", appex])
        _ = run(pluginkit, ["-a", appex])
        _ = run(pluginkit, ["-e", "use", "-i", extensionID])

        tell("TEMPRIX is running",
             "Every change to files in your projects is now saved.\n\n"
                + "To see a file's earlier versions, right-click it in Finder and choose Show TEMPRIX History.")
        NSApp.terminate(nil)
    }

    /// Whether the launchd agent is set up to run this copy's CLI.
    private func daemonRuns(_ cli: URL) -> Bool {
        let plist = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/LaunchAgents/com.temprix.daemon.plist")
        guard let data = try? Data(contentsOf: plist),
              let dict = try? PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any],
              let program = (dict["ProgramArguments"] as? [String])?.first
        else { return false }
        // A source install points launchd at ~/.local/bin/temprix, a link
        // into the app; compare where both actually lead.
        return URL(fileURLWithPath: program).resolvingSymlinksInPath() == cli.resolvingSymlinksInPath()
    }

    private func tell(_ title: String, _ message: String) {
        if #available(macOS 14, *) {
            NSApp.activate()
        } else {
            NSApp.activate(ignoringOtherApps: true)
        }
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = message
        alert.runModal()
    }

    // ─── temprix:// URLs ────────────────────────────────────────────────

    private func handle(_ url: URL) {
        guard url.scheme == "temprix", url.host == "reveal",
              let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems
        else {
            log.error("ignoring unrecognised URL: \(url.absoluteString, privacy: .public)")
            return
        }
        let paths = items.filter { $0.name == "path" }.compactMap(\.value)
        guard !paths.isEmpty else { return }

        // The CLI ships inside this bundle (Contents/MacOS/temprix), so the
        // app and the CLI it calls are always the same build.
        guard let cli = Bundle.main.url(forAuxiliaryExecutable: "temprix") else {
            fail("bundled CLI missing from TEMPRIX.app; reinstall it")
            return
        }
        let result = run(cli, ["reveal"] + paths)
        if result.status != 0 {
            fail(result.output)
        }
    }

    /// The standard alert sound says "nothing to show" without a banner.
    /// The reason goes to the log:
    /// `log show --predicate 'subsystem == "com.temprix.app"'`.
    private func fail(_ message: String) {
        log.error("\(message, privacy: .public)")
        NSSound.beep()
    }

    // ─── Helpers ────────────────────────────────────────────────────────

    /// Run a program to completion. Returns its exit status and stderr,
    /// with the CLI's terminal colour codes removed.
    private func run(_ executable: URL, _ arguments: [String]) -> (status: Int32, output: String) {
        let process = Process()
        process.executableURL = executable
        process.arguments = arguments
        let stderr = Pipe()
        process.standardError = stderr
        process.standardOutput = FileHandle.nullDevice
        do {
            try process.run()
        } catch {
            return (-1, error.localizedDescription)
        }
        // Read before waiting: a child that fills the pipe buffer would
        // otherwise block forever on a parent that isn't reading.
        let data = stderr.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        let text = String(decoding: data, as: UTF8.self)
            .replacingOccurrences(of: "\u{1B}\\[[0-9;]*m", with: "", options: .regularExpression)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return (process.terminationStatus, text)
    }

    private func scheduleExit() {
        idleTimer?.invalidate()
        idleTimer = Timer.scheduledTimer(withTimeInterval: idleExit, repeats: false) { _ in
            NSApp.terminate(nil)
        }
    }
}

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.run()
