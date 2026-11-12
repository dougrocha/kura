import AppKit
import ApplicationServices

/// Pastes into the app in front by pressing ⌘V for you. That takes
/// Accessibility permission (System Settings → Privacy & Security →
/// Accessibility); without it, picking just copies.
///
/// Kura is signed ad hoc, so macOS may forget the permission after a
/// rebuild (`just install`); turn it off and on again there if pasting stops.
@MainActor
enum Paster {
    static var canPaste: Bool { AXIsProcessTrusted() }

    /// Shows the system's "Kura would like to control this computer" prompt
    /// (once; after that, macOS only lists Kura in Settings).
    static func askForPermission() {
        _ = AXIsProcessTrustedWithOptions(["AXTrustedCheckOptionPrompt": true] as CFDictionary)
    }

    /// ⌘V to the app in front, a moment after the launcher has closed so the
    /// keystroke lands in that app, not in the launcher. Without permission it
    /// asks for it instead; what was copied stays on the clipboard.
    static func paste(after delay: Duration = .milliseconds(80)) {
        guard canPaste else { return askForPermission() }
        Task {
            try? await Task.sleep(for: delay)
            let source = CGEventSource(stateID: .combinedSessionState)
            let v = CGKeyCode(9)  // kVK_ANSI_V
            for down in [true, false] {
                let event = CGEvent(keyboardEventSource: source, virtualKey: v, keyDown: down)
                event?.flags = .maskCommand
                event?.post(tap: .cghidEventTap)
            }
        }
    }
}
