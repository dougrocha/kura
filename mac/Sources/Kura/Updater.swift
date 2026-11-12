import AppKit
import Observation
import SwiftUI

/// Notices when Kura.app is replaced on disk (`just install`) and relaunches
/// into the new build, with Rex's flow: "Update available" and Download, a
/// spinner while it downloads, then Relaunch (or, with "Relaunch when
/// hidden" on, the next time Kura is hidden).
@MainActor @Observable
final class Updater {
    enum Phase: Equatable {
        case current
        /// After Check for Updates finds nothing; hides itself.
        case upToDate
        /// A new build is on disk; Download is waiting for a click.
        case available
        /// Pretend download, so it feels like a real update. Builds are local.
        case downloading
        case ready
        case waitingForIdle
        case relaunching
    }

    private(set) var phase = Phase.current
    /// The toast was closed. The update keeps its phase; Check for Updates shows it again.
    private(set) var isHidden = false
    /// `mac.relaunch_when_hidden` from the shared config; set by the app.
    @ObservationIgnored var relaunchWhenHidden: () -> Bool = { true }

    private let bundleURL = Bundle.main.bundleURL
    private let launchedBuild = Updater.build(at: Bundle.main.bundleURL)

    /// A new build was seen on disk.
    private var found: Bool { phase != .current && phase != .upToDate }

    func start() {
        guard launchedBuild != nil else { return }  // `swift run`: no bundle to watch
        // Polling one small file is simpler than watching the folder, and
        // `just install` swaps the bundle with `mv`, so it never sees half of one.
        Task {
            while !found {
                try? await Task.sleep(for: .seconds(2))
                checkDisk()
            }
        }
        Task {
            for await _ in NotificationCenter.default.notifications(named: NSApplication.didHideNotification)
            where phase == .waitingForIdle || (phase == .ready && relaunchWhenHidden()) {
                relaunch(activate: false)
            }
        }
    }

    /// Kura → Check for Updates…: shows the update if there is one (even after
    /// the toast was closed), or "Kura is up to date" for a moment.
    func checkForUpdates() {
        isHidden = false
        guard !found, !checkDisk() else { return }
        phase = .upToDate
        Task {
            try? await Task.sleep(for: .seconds(3))
            if phase == .upToDate { phase = .current }
        }
    }

    /// Download: a spinner for a moment (the new build is already on disk),
    /// then Relaunch.
    func download() {
        guard phase == .available else { return }
        phase = .downloading
        Task {
            try? await Task.sleep(for: .milliseconds(Int.random(in: 1200...2000)))
            if phase == .downloading { phase = .ready }
        }
    }

    /// Hides the toast. The update stays where it was.
    func dismiss() {
        if phase == .upToDate { phase = .current } else { isHidden = true }
    }

    /// Shows "Update available" when a different build is on disk.
    @discardableResult private func checkDisk() -> Bool {
        guard !found, let launchedBuild, let onDisk = Self.build(at: bundleURL), onDisk != launchedBuild else {
            return found
        }
        phase = .available
        isHidden = false
        return true
    }

    func relaunchWhenIdle() {
        phase = .waitingForIdle
        isHidden = false
    }

    /// Waits for this process to exit, then opens the new bundle. Our server
    /// stops with us (`--exit-with-stdin`), and the new app starts its own.
    func relaunch(activate: Bool = true) {
        phase = .relaunching
        let pid = ProcessInfo.processInfo.processIdentifier
        let task = Process()
        task.executableURL = URL(filePath: "/bin/sh")
        task.arguments = [
            "-c", "while kill -0 \(pid) 2>/dev/null; do sleep 0.1; done; open \(activate ? "" : "-g ")\"$0\"",
            bundleURL.path,
        ]
        do {
            try task.run()
            Task {
                try? await Task.sleep(for: .milliseconds(400))  // let "Relaunching" show
                NSApp.terminate(nil)
            }
        } catch {
            phase = .ready
        }
    }

    /// From disk, not `Bundle.main`, which keeps the Info.plist it launched with.
    private static func build(at url: URL) -> String? {
        let plist = url.appending(path: "Contents/Info.plist")
        guard let data = try? Data(contentsOf: plist),
            let info = try? PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any]
        else { return nil }
        return info["CFBundleVersion"] as? String
    }
}

/// A toast in the bottom-right corner, like Rex's: a round status icon, one
/// line, and buttons. Hold ⌥ to relaunch once Kura is hidden instead.
struct UpdatePill: View {
    @Environment(Updater.self) private var updater
    @State private var optionHeld = false

    var body: some View {
        Group {
            if updater.phase != .current && !updater.isHidden {
                HStack(spacing: 10) {
                    StatusIcon(phase: updater.phase)
                    Text(title)
                        .font(.system(size: 13, weight: .medium))
                        .contentTransition(.interpolate)
                        .lineLimit(1)
                    if updater.phase == .available {
                        PillButton(title: "Download") { updater.download() }
                    } else if updater.phase == .ready {
                        PillButton(title: optionHeld ? "Later" : "Relaunch") {
                            if optionHeld { updater.relaunchWhenIdle() } else { updater.relaunch() }
                        }
                        .help("Hold ⌥ to relaunch when Kura is hidden")
                    } else if updater.phase == .waitingForIdle {
                        // Waiting is the polite default, but you can always skip it.
                        PillButton(title: "Relaunch") { updater.relaunch() }
                            .help("Kura also relaunches by itself the next time you hide it (⌘H)")
                    }
                    if updater.phase != .relaunching && updater.phase != .upToDate {
                        Button { updater.dismiss() } label: {
                            Image(systemName: "xmark")
                                .font(.system(size: 9, weight: .semibold))
                                .foregroundStyle(.secondary)
                                .frame(width: 22, height: 22)
                                .background(.quaternary, in: .circle)
                        }
                        .buttonStyle(.plain)
                        .help("Hide")
                    }
                }
                .padding(.leading, 7)
                .padding(.trailing, 5)
                .padding(.vertical, 5)
                .glassEffect(.regular, in: .capsule)
                .padding(12)
                .transition(.opacity.combined(with: .offset(y: 8)))
            }
        }
        .animation(.smooth, value: updater.phase)
        .animation(.smooth(duration: 0.15), value: optionHeld)
        .onModifierKeysChanged(mask: .option) { _, keys in optionHeld = keys.contains(.option) }
    }

    private var title: String {
        switch updater.phase {
        case .current, .available: "Kura update available"
        case .downloading: "Downloading update…"
        case .upToDate: "Kura is up to date"
        case .ready: "Kura is ready to update"
        case .waitingForIdle: "Kura will relaunch when hidden"
        case .relaunching: "Relaunching"
        }
    }
}

/// A quiet text button that matches the × beside it: same height, same grey
/// fill, like Rex's toast (no accent color inside the pill).
private struct PillButton: View {
    let title: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(.system(size: 12, weight: .medium))
                .padding(.horizontal, 10)
                .frame(height: 22)
                .background(.quaternary, in: .capsule)
                .contentShape(.capsule)
        }
        .buttonStyle(.plain)
    }
}

/// The round icon: a spinner while working, else a solid dot with a symbol.
private struct StatusIcon: View {
    let phase: Updater.Phase

    var body: some View {
        ZStack {
            switch phase {
            case .downloading, .relaunching:
                ProgressView().controlSize(.small)
            case .upToDate:
                Circle().fill(.primary)
                Image(systemName: "checkmark")
                    .font(.system(size: 9, weight: .bold))
                    .foregroundStyle(.background)
            default:
                Circle().fill(.primary)
                Image(systemName: phase == .waitingForIdle ? "clock" : "arrow.down")
                    .font(.system(size: 9, weight: .bold))
                    .foregroundStyle(.background)
            }
        }
        .frame(width: 18, height: 18)
        .transition(.opacity)
    }
}
