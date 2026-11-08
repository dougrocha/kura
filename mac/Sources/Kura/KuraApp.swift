import AppKit
import SwiftUI

@main
struct KuraApp: App {
    @NSApplicationDelegateAdaptor private var appDelegate: AppDelegate
    @State private var library = LibraryModel(client: KuraClient.fromEnvironment())

    var body: some Scene {
        WindowGroup("Kura") {
            ContentView()
                .environment(library)
                .frame(minWidth: 700, minHeight: 450)
        }
        .commands {
            CommandGroup(after: .toolbar) {
                Button("Reload") { Task { await library.load() } }
                    .keyboardShortcut("r")
            }
        }
    }
}

/// Run from `swift run` there's no app bundle, so macOS treats the process as a
/// background tool: no Dock icon, and the window opens behind the terminal.
/// Make it a regular app and bring it to the front.
final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        NSApp.activate()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }
}
