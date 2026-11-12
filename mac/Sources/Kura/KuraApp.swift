import AppKit
import SwiftUI

@main
struct KuraApp: App {
    @NSApplicationDelegateAdaptor private var appDelegate: AppDelegate

    var body: some Scene {
        WindowGroup("Kura") {
            ContentView()
                .environment(appDelegate.library)
                .environment(appDelegate.updater)
                .frame(minWidth: 760, minHeight: 480)
        }
        .windowToolbarStyle(.unified)
        .commands {
            CommandGroup(after: .appInfo) {
                Button("Check for Updates…") { appDelegate.updater.checkForUpdates() }
            }
            CommandGroup(replacing: .newItem) {
                // The shortcut is global (it works from any app), so it's in the title,
                // not a menu key equivalent.
                Button("Quick Pick  \(HotKey.quickPick.label)") { appDelegate.launcher.show() }
                Button("Add Images…") { appDelegate.library.requestAdd() }
                    .keyboardShortcut("o")
            }
            CommandGroup(after: .textEditing) {
                Button("Find") { appDelegate.library.focusSearch() }
                    .keyboardShortcut("f")
            }
            ImageCommands(library: appDelegate.library)
            CommandGroup(after: .toolbar) {
                Button("Reload") { Task { await appDelegate.library.load() } }
                    .keyboardShortcut("r")
            }
        }

        Settings {
            SettingsView()
                .environment(appDelegate.library)
                .environment(appDelegate.updater)
        }
    }
}

/// Edit the selected image. Off while the search field has focus, where ⌘E and
/// ⌘⌫ belong to the text field.
struct ImageCommands: Commands {
    let library: LibraryModel

    private var enabled: Bool { library.selected != nil && !library.isSearching }

    var body: some Commands {
        CommandMenu("Image") {
            Button("Rename…") { library.requestRename() }
                .keyboardShortcut("e")
                .disabled(!enabled)
            Button("Delete…") { library.requestDelete() }
                .keyboardShortcut(.delete, modifiers: .command)
                .disabled(!enabled)
        }
    }
}

/// Owns the model so quitting can stop the app's server.
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    let library = LibraryModel()
    let updater = Updater()
    lazy var launcher = LauncherPanel(launcher: Launcher(root: MemesPage(library: library)))

    func applicationDidFinishLaunching(_ notification: Notification) {
        // Under `swift run` there's no app bundle, so macOS treats the process
        // as a background tool. Make it a regular app in front. (No-op in Kura.app.)
        NSApp.setActivationPolicy(.regular)
        NSApp.activate()
        updater.relaunchWhenHidden = { [library] in library.config?.relaunchWhenHidden ?? true }
        updater.start()
        if !launcher.registerHotKey() {
            print("Kura: \(HotKey.quickPick.label) is taken by another app; Quick Pick is in the File menu")
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        library.shutdown()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }
}
