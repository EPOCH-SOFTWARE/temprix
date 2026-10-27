// Finder Sync extension: adds "Show TEMPRIX History" to the top level of
// Finder's right-click menu. Quick Actions can only appear in a submenu;
// a Finder Sync extension is the only supported way onto the main menu.
//
// macOS forces app extensions into the App Sandbox, so this process cannot
// run the temprix CLI or read ~/.temprix. It forwards the selection as a
// temprix:// URL to the host app (macos/App), which runs `temprix reveal`.

import Cocoa
import FinderSync
import os

private let log = Logger(subsystem: "com.temprix.app", category: "finder-extension")

@objc(FinderSync)
final class FinderSync: FIFinderSync {
    override init() {
        super.init()
        // Offer the menu everywhere. Items without history open nothing;
        // the host app plays the alert sound.
        FIFinderSyncController.default().directoryURLs = [URL(fileURLWithPath: "/")]
        log.info("loaded")
    }

    override func menu(for menuKind: FIMenuKind) -> NSMenu? {
        // Items: right-click on files or folders.
        // Container: right-click on a window's background — that folder's
        // history, which is where versions of deleted files live.
        guard menuKind == .contextualMenuForItems || menuKind == .contextualMenuForContainer else {
            return nil
        }
        let menu = NSMenu(title: "")
        let item = NSMenuItem(
            title: "Show TEMPRIX History",
            action: #selector(showHistory(_:)),
            keyEquivalent: ""
        )
        item.image = Self.menuIcon()
        menu.addItem(item)
        return menu
    }

    /// Finder recolours its own menu icons to match the menu text, but shows
    /// an extension's image as-is: a standard SF Symbol came out solid black
    /// on the dark menu. So draw the symbol in the menu's text colour for
    /// the current appearance, at the menu font's size. Rebuilt on every
    /// right-click, so it follows a light/dark switch.
    private static func menuIcon() -> NSImage? {
        let config = NSImage.SymbolConfiguration(pointSize: NSFont.menuFont(ofSize: 0).pointSize, weight: .regular)
        guard let symbol = NSImage(systemSymbolName: "clock.arrow.circlepath", accessibilityDescription: nil)?
            .withSymbolConfiguration(config)
        else { return nil }

        let size = symbol.size
        let rect = NSRect(origin: .zero, size: size)
        let appearance = NSApplication.shared.effectiveAppearance
        let image = NSImage(size: size)
        // Explicit 1x and 2x bitmaps: what Finder receives is pixels, so
        // nothing is left for it to tint or resolve later.
        for scale in [1.0, 2.0] {
            guard let rep = NSBitmapImageRep(
                bitmapDataPlanes: nil,
                pixelsWide: Int(size.width * scale), pixelsHigh: Int(size.height * scale),
                bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
            ) else { continue }
            rep.size = size
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
            symbol.draw(in: rect)
            appearance.performAsCurrentDrawingAppearance {
                NSColor.labelColor.set()
                rect.fill(using: .sourceAtop)
            }
            NSGraphicsContext.restoreGraphicsState()
            image.addRepresentation(rep)
        }
        return image
    }

    @objc func showHistory(_ sender: AnyObject?) {
        let controller = FIFinderSyncController.default()
        let selected = controller.selectedItemURLs() ?? []
        let targets = selected.isEmpty ? [controller.targetedURL()].compactMap { $0 } : selected
        guard !targets.isEmpty else { return }

        var components = URLComponents()
        components.scheme = "temprix"
        components.host = "reveal"
        components.queryItems = targets.map { URLQueryItem(name: "path", value: $0.path) }
        guard let url = components.url else {
            log.error("could not encode selection as URL")
            return
        }
        log.info("forwarding \(targets.count) item(s) to host app")
        NSWorkspace.shared.open(url)
    }
}
