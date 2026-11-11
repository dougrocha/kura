import AppKit
import SwiftUI

@main
struct KuraApp: App {
    @NSApplicationDelegateAdaptor private var appDelegate: AppDelegate

    var body: some Scene {
        WindowGroup("Kura") {
            ContentView()
                .environment(appDelegate.library)
                .frame(minWidth: 760, minHeight: 480)
        }
        .windowToolbarStyle(.unified)
        .commands {
            CommandGroup(after: .textEditing) {
                Button("Find") { appDelegate.library.focusSearch() }
                    .keyboardShortcut("f")
            }
            CommandGroup(after: .toolbar) {
                Button("Reload") { Task { await appDelegate.library.load() } }
                    .keyboardShortcut("r")
            }
        }
    }
}

/// Owns the model so quitting can stop the app's server.
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    let library = LibraryModel()

    func applicationDidFinishLaunching(_ notification: Notification) {
        // Under `swift run` there's no app bundle, so macOS treats the process
        // as a background tool. Make it a regular app in front. (No-op in Kura.app.)
        NSApp.setActivationPolicy(.regular)
        NSApp.activate()
    }

    func applicationWillTerminate(_ notification: Notification) {
        library.shutdown()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }
}
