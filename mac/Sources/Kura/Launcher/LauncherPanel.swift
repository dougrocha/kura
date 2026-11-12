import AppKit
import SwiftUI

/// The floating window a `Launcher` lives in: opened by a global hotkey over
/// whatever app is in front, without activating Kura, so that app is still
/// in front to paste into when the launcher closes.
@MainActor
final class LauncherPanel: NSObject, NSWindowDelegate {
    let launcher: Launcher
    private var hotKey: HotKey?

    private lazy var panel: NSPanel = {
        let panel = Panel(
            contentRect: NSRect(origin: .zero, size: LauncherView.size),
            styleMask: [.nonactivatingPanel, .titled, .fullSizeContentView], backing: .buffered, defer: true)
        panel.titlebarAppearsTransparent = true
        panel.titleVisibility = .hidden
        for button in [NSWindow.ButtonType.closeButton, .miniaturizeButton, .zoomButton] {
            panel.standardWindowButton(button)?.isHidden = true
        }
        panel.isFloatingPanel = true
        panel.level = .floating
        panel.hidesOnDeactivate = false
        panel.isMovableByWindowBackground = true
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.delegate = self
        panel.contentView = NSHostingView(rootView: LauncherView(launcher: launcher))
        return panel
    }()

    init(launcher: Launcher) {
        self.launcher = launcher
        super.init()
        launcher.close = { [weak self] in self?.close() }
    }

    /// Listen for the hotkey. Returns false if another app already owns it.
    @discardableResult
    func registerHotKey() -> Bool {
        let key = HotKey.quickPick
        hotKey = HotKey(keyCode: key.keyCode, modifiers: key.modifiers) { [weak self] in self?.toggle() }
        return hotKey != nil
    }

    func toggle() {
        panel.isVisible ? close() : show()
    }

    func show() {
        launcher.opened()
        // Upper third of the screen with the mouse, like Spotlight.
        let screen = NSScreen.screens.first { $0.frame.contains(NSEvent.mouseLocation) } ?? NSScreen.main
        if let frame = screen?.visibleFrame {
            let size = panel.frame.size
            panel.setFrameOrigin(NSPoint(x: frame.midX - size.width / 2, y: frame.minY + frame.height * 0.6 - size.height / 2))
        }
        panel.makeKeyAndOrderFront(nil)
    }

    func close() {
        guard panel.isVisible else { return }
        panel.orderOut(nil)
        launcher.closed()
    }

    /// Clicking anywhere else dismisses it, like Spotlight.
    func windowDidResignKey(_ notification: Notification) {
        close()
    }

    /// A panel that looks borderless still has to accept typing.
    private final class Panel: NSPanel {
        override var canBecomeKey: Bool { true }
        override var canBecomeMain: Bool { false }
    }
}
