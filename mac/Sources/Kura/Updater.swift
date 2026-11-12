import AppKit
import Observation
import SwiftUI

/// Notices when Kura.app is replaced on disk (`just install`) and relaunches
/// into the new build: right away, or once Kura is hidden.
@MainActor @Observable
final class Updater {
    enum Phase: Equatable {
        case current
        /// After Check for Updates finds nothing; hides itself.
        case upToDate
        /// Pretend download, so it feels like a real update. Builds are local.
        case downloading(Double)
        case ready
        case waitingForIdle
        case relaunching
    }

    private(set) var phase = Phase.current
    /// `mac.relaunch_when_hidden` from the shared config; set by the app.
    @ObservationIgnored var relaunchWhenHidden: () -> Bool = { true }

    private let bundleURL = Bundle.main.bundleURL
    private let launchedBuild = Updater.build(at: Bundle.main.bundleURL)
    private var found = false

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
            where phase == .waitingForIdle {
                relaunch(activate: false)
            }
        }
    }

    /// Kura → Check for Updates…: shows the update if there is one (even after
    /// the toast was closed), or "Kura is up to date" for a moment.
    func checkForUpdates() {
        switch phase {
        case .current, .upToDate:
            if found {
                phase = .ready
            } else if !checkDisk() {
                phase = .upToDate
                Task {
                    try? await Task.sleep(for: .seconds(3))
                    if phase == .upToDate { phase = .current }
                }
            }
        default: break
        }
    }

    /// Hides the toast. The update stays found; Check for Updates brings it back.
    func dismiss() {
        if phase != .relaunching { phase = .current }
    }

    /// Starts the "download" when a different build is on disk.
    @discardableResult private func checkDisk() -> Bool {
        guard !found, let launchedBuild, let onDisk = Self.build(at: bundleURL), onDisk != launchedBuild else {
            return found
        }
        found = true
        Task {
            for step in 0...40 {
                phase = .downloading(Double(step) / 40)
                try? await Task.sleep(for: .milliseconds(Int.random(in: 40...140)))
            }
            phase = relaunchWhenHidden() ? .waitingForIdle : .ready
        }
        return true
    }

    func relaunchWhenIdle() {
        phase = .waitingForIdle
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
            if updater.phase != .current {
                HStack(spacing: 10) {
                    StatusIcon(phase: updater.phase)
                    Text(title)
                        .font(.system(size: 13, weight: .medium))
                        .contentTransition(.interpolate)
                        .lineLimit(1)
                    if updater.phase == .ready {
                        Button(optionHeld ? "Later" : "Relaunch") {
                            if optionHeld { updater.relaunchWhenIdle() } else { updater.relaunch() }
                        }
                        .buttonStyle(.glassProminent)
                        .controlSize(.small)
                        .help("Hold ⌥ to relaunch when Kura is hidden")
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
        case .current, .downloading: "Downloading update"
        case .upToDate: "Kura is up to date"
        case .ready: "Kura is ready to update"
        case .waitingForIdle: "Kura will relaunch when hidden"
        case .relaunching: "Relaunching"
        }
    }
}

/// The round icon: a filling ring while downloading, then a solid dot.
private struct StatusIcon: View {
    let phase: Updater.Phase

    var body: some View {
        ZStack {
            switch phase {
            case .downloading(let progress):
                Circle().stroke(.quaternary, lineWidth: 1.5)
                Circle()
                    .trim(from: 0, to: progress)
                    .stroke(.primary, style: StrokeStyle(lineWidth: 1.5, lineCap: .round))
                    .rotationEffect(.degrees(-90))
                    .animation(.linear(duration: 0.12), value: progress)
            case .relaunching:
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
