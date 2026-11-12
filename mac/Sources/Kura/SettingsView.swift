import AppKit
import SwiftUI

/// Kura → Settings… (⌘,), laid out like System Settings: a searchable
/// sidebar of panes, each a grouped form. Everything here is the shared
/// config file, so `kura config` in a terminal changes the same settings.
struct SettingsView: View {
    @Environment(LibraryModel.self) private var library
    @State private var pane: Pane? = .server
    @State private var search = ""

    enum Pane: String, CaseIterable, Identifiable {
        case server, general, library, updates, configFile

        var id: Self { self }

        var title: String {
            switch self {
            case .server: "Server"
            case .general: "General"
            case .library: "Library"
            case .updates: "Updates"
            case .configFile: "Config File"
            }
        }

        var symbol: String {
            switch self {
            case .server: "archivebox.fill"
            case .general: "gearshape.fill"
            case .library: "photo.on.rectangle"
            case .updates: "arrow.down"
            case .configFile: "terminal.fill"
            }
        }

        var color: Color {
            switch self {
            case .server: .indigo
            case .general: .gray
            case .library: .orange
            case .updates: .blue
            case .configFile: Color(white: 0.2)
            }
        }

        /// What search matches besides the title.
        var keywords: String {
            switch self {
            case .server: "restart running address port"
            case .general: "copy close hide pick return double-click"
            case .library: "folder images location finder"
            case .updates: "version relaunch install check"
            case .configFile: "toml terminal cli kura config edit"
            }
        }
    }

    private var panes: [Pane] {
        let words = search.lowercased().split(separator: " ")
        return Pane.allCases.filter { pane in
            let text = "\(pane.title) \(pane.keywords)".lowercased()
            return words.allSatisfy { text.contains($0) }
        }
    }

    var body: some View {
        NavigationSplitView {
            List(selection: $pane) {
                if panes.contains(.server) {
                    ServerSidebarRow().tag(Pane.server)
                }
                Section {
                    ForEach(panes.filter { $0 != .server }) { pane in
                        Label {
                            Text(pane.title)
                        } icon: {
                            SettingsIcon(symbol: pane.symbol, color: pane.color)
                        }
                        .tag(pane)
                    }
                }
            }
            .searchable(text: $search, placement: .sidebar, prompt: "Search Settings")
            .navigationSplitViewColumnWidth(220)
            .toolbar(removing: .sidebarToggle)
        } detail: {
            Group {
                if let pane {
                    detail(for: pane)
                } else {
                    ContentUnavailableView.search(text: search)
                }
            }
            .navigationTitle(pane?.title ?? "Settings")
        }
        .frame(width: 720, height: 500)
        .onChange(of: search) { if let first = panes.first, !panes.contains(pane ?? first) { pane = first } }
        .task { await library.refreshConfig() }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            Task { await library.refreshConfig() }
        }
    }

    @ViewBuilder private func detail(for pane: Pane) -> some View {
        switch pane {
        case .server: ServerPane()
        case .updates: UpdatesPane()
        case .general, .library, .configFile:
            if let config = library.config {
                switch pane {
                case .general: GeneralPane(config: config)
                case .library: LibraryPane(config: config)
                default: ConfigFilePane(config: config)
                }
            } else {
                ContentUnavailableView(
                    "Settings Aren't Available",
                    systemImage: "gearshape",
                    description: Text(
                        library.serverStartedAt == nil && library.client != nil
                            ? "Kura is connected to a server it didn't start. Use `kura config` instead."
                            : "Kura's server isn't running."
                    )
                )
            }
        }
    }
}

// MARK: - Panes

private struct ServerPane: View {
    @Environment(LibraryModel.self) private var library

    var body: some View {
        Form {
            Section {
                HStack(spacing: 12) {
                    SettingsIcon(symbol: "archivebox.fill", color: .indigo, size: 40)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Kura Server").font(.headline)
                        ServerStatus().foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("Restart") { Task { await library.restartServer() } }
                        .disabled(library.serverStartedAt == nil && library.error == nil)
                }
                .padding(.vertical, 4)

                if let client = library.client {
                    LabeledContent {
                        Button {
                            NSPasteboard.general.clearContents()
                            NSPasteboard.general.setString(client.baseURL.absoluteString, forType: .string)
                        } label: {
                            Image(systemName: "doc.on.doc")
                        }
                        .help("Copy Address")
                    } label: {
                        Text("Address")
                        Text(client.baseURL.absoluteString).textSelection(.enabled)
                    }
                }
            } footer: {
                Text("Kura runs its own server on this Mac and stops it when you quit.")
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .formStyle(.grouped)
    }
}

private struct GeneralPane: View {
    @Environment(LibraryModel.self) private var library
    let config: KuraConfig

    var body: some View {
        Form {
            Section("Copying") {
                Toggle(isOn: setting("pick.close_after_copy", config.closeAfterCopy)) {
                    Text("Close after copying")
                    Text("Return or double-click hides Kura, and `kura pick` exits, so you can paste right away.")
                }
            }
            ConfigErrorSection()
        }
        .formStyle(.grouped)
    }

    private func setting(_ key: String, _ value: Bool) -> Binding<Bool> {
        Binding(get: { value }, set: { new in Task { await library.setConfig(key, to: new ? "true" : "false") } })
    }
}

private struct LibraryPane: View {
    @Environment(LibraryModel.self) private var library
    let config: KuraConfig

    var body: some View {
        Form {
            Section {
                HStack(spacing: 12) {
                    Image(nsImage: NSWorkspace.shared.icon(forFile: config.libraryFolder))
                        .resizable()
                        .frame(width: 40, height: 40)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("\(library.images.count) images").font(.headline)
                        Text(tildePath(config.libraryFolder))
                            .foregroundStyle(.secondary)
                            .truncationMode(.middle)
                            .lineLimit(1)
                            .textSelection(.enabled)
                    }
                    Spacer()
                    Button("Show in Finder") {
                        NSWorkspace.shared.selectFile(nil, inFileViewerRootedAtPath: config.libraryFolder)
                    }
                }
                .padding(.vertical, 4)
            }

            Section {
                LabeledContent {
                    HStack {
                        if config.library != nil {
                            Button("Use Default") { Task { await library.setConfig("library", to: nil) } }
                        }
                        Button("Choose…", action: choose)
                    }
                } label: {
                    Text("Library folder")
                    Text(config.library.map { $0 } ?? "Default: ~/Pictures/kura")
                }
            } footer: {
                Text("A chosen folder holds the images, database and previews together. Changing it restarts the server and doesn't move existing images.")
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            ConfigErrorSection()
        }
        .formStyle(.grouped)
    }

    private func choose() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.prompt = "Use Folder"
        panel.message = "Choose a folder for your Kura library."
        guard panel.runModal() == .OK, let url = panel.url else { return }
        Task { await library.setConfig("library", to: tildePath(url.path(percentEncoded: false))) }
    }
}

private struct UpdatesPane: View {
    @Environment(LibraryModel.self) private var library
    @Environment(Updater.self) private var updater

    var body: some View {
        Form {
            Section {
                HStack(spacing: 12) {
                    SettingsIcon(symbol: "arrow.down", color: .blue, size: 40)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Kura \(Self.info("CFBundleShortVersionString") ?? "dev")").font(.headline)
                        Text(status).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button("Check for Updates") { updater.checkForUpdates() }
                }
                .padding(.vertical, 4)
            }

            if let config = library.config {
                Section {
                    Toggle(
                        isOn: Binding(
                            get: { config.relaunchWhenHidden },
                            set: { new in
                                Task { await library.setConfig("mac.relaunch_when_hidden", to: new ? "true" : "false") }
                            }
                        )
                    ) {
                        Text("Install updates automatically")
                        Text("Kura relaunches into a new build once it's hidden.")
                    }
                }
                ConfigErrorSection()
            }
        }
        .formStyle(.grouped)
    }

    private var status: String {
        switch updater.phase {
        case .downloading: "Downloading an update"
        case .ready: "An update is ready"
        case .waitingForIdle: "Relaunches when hidden"
        case .relaunching: "Relaunching"
        case .current, .upToDate: Self.info("CFBundleVersion").map { "Build \($0)" } ?? "Running from source"
        }
    }

    private static func info(_ key: String) -> String? {
        Bundle.main.infoDictionary?[key] as? String
    }
}

private struct ConfigFilePane: View {
    let config: KuraConfig

    var body: some View {
        Form {
            Section {
                LabeledContent {
                    HStack {
                        Button("Open") { NSWorkspace.shared.open(URL(filePath: config.path)) }
                            .disabled(!FileManager.default.fileExists(atPath: config.path))
                        Button("Show in Finder") {
                            NSWorkspace.shared.activateFileViewerSelecting([URL(filePath: config.path)])
                        }
                        .disabled(!FileManager.default.fileExists(atPath: config.path))
                    }
                } label: {
                    Text("Config file")
                    Text(tildePath(config.path)).textSelection(.enabled)
                }
            } footer: {
                Text("Settings and the `kura` command share this file. It's created the first time you change a setting.")
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }

            Section("From the Terminal") {
                command("kura config", "Show every setting")
                command("kura config set pick.close_after_copy false", "Change one")
                command("kura config edit", "Open the file in $EDITOR")
            }
        }
        .formStyle(.grouped)
    }

    private func command(_ text: String, _ about: String) -> some View {
        LabeledContent {
            Text(text).font(.system(.body, design: .monospaced)).textSelection(.enabled)
        } label: {
            Text(about)
        }
    }
}

// MARK: - Pieces

/// The sidebar's first row, like Rex's: the server and whether it's running.
private struct ServerSidebarRow: View {
    var body: some View {
        HStack(spacing: 10) {
            SettingsIcon(symbol: "archivebox.fill", color: .indigo, size: 32)
            VStack(alignment: .leading, spacing: 1) {
                Text("Kura Server").font(.headline)
                ServerStatus(short: true).font(.subheadline).foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 4)
    }
}

private struct ServerStatus: View {
    @Environment(LibraryModel.self) private var library
    var short = false

    var body: some View {
        TimelineView(.periodic(from: .now, by: 30)) { context in
            Text(text(now: context.date))
        }
    }

    private func text(now: Date) -> String {
        if library.error != nil { return "Stopped" }
        if library.client != nil && library.serverStartedAt == nil { return "External server" }
        guard let started = library.serverStartedAt else { return "Starting…" }
        if short { return "Running" }
        let seconds = now.timeIntervalSince(started)
        if seconds < 60 { return "Running, just started" }
        let duration = Duration.seconds(seconds)
            .formatted(.units(allowed: [.days, .hours, .minutes], width: .narrow, maximumUnitCount: 2))
        return "Running for \(duration)"
    }
}

/// System Settings' icon: a white symbol on a colored rounded square.
private struct SettingsIcon: View {
    let symbol: String
    let color: Color
    var size: CGFloat = 22

    var body: some View {
        RoundedRectangle(cornerRadius: size * 0.27, style: .continuous)
            .fill(color.gradient)
            .overlay {
                Image(systemName: symbol)
                    .font(.system(size: size * 0.5, weight: .semibold))
                    .foregroundStyle(.white)
            }
            .frame(width: size, height: size)
    }
}

/// Why the last change didn't save, under the pane's settings.
private struct ConfigErrorSection: View {
    @Environment(LibraryModel.self) private var library

    var body: some View {
        if let message = library.configError {
            Section {
                Label(message, systemImage: "exclamationmark.triangle.fill")
                    .foregroundStyle(.red)
            }
        }
    }
}

private func tildePath(_ path: String) -> String {
    let home = NSHomeDirectory()
    return path.hasPrefix(home + "/") || path == home ? "~" + path.dropFirst(home.count) : path
}
