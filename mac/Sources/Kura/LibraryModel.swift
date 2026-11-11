import AppKit
import Observation

/// What the window shows, and the connection behind it.
///
/// Normally the app starts its own server (`ServerProcess`). Set `KURA_URL` to
/// talk to a server you run yourself instead; the app then starts nothing.
@MainActor
@Observable
final class LibraryModel {
    private(set) var client: KuraClient?
    private(set) var images: [KuraImage] = []
    private(set) var error: String?
    private(set) var isLoading = false
    /// Connected to the server's change feed, so the list updates by itself.
    private(set) var isLive = false
    var selection: KuraImage.ID?
    /// The sidebar's search, with the TUI's rules (see `SearchQuery`).
    var searchText = "" {
        didSet { refreshVisible() }
    }
    /// The images the sidebar shows: all of them, or the search's matches, best first.
    private(set) var visibleImages: [KuraImage] = []
    /// Bumped by Find (⌘F) and `/`; the view focuses the search field.
    private(set) var searchRequests = 0
    /// Shown briefly after copying ("Copied cat jam").
    private(set) var notice: String?

    @ObservationIgnored private let server = ServerProcess()
    /// Bumped by every copy, so a slow download can't overwrite a newer copy.
    @ObservationIgnored private var copyGeneration = 0
    @ObservationIgnored private var noticeTask: Task<Void, Never>?

    init() {
        server.onUnexpectedExit = { [weak self] message in
            self?.client = nil
            self?.isLive = false
            self?.error = message
        }
    }

    var selected: KuraImage? {
        visibleImages.first { $0.id == selection }
    }

    /// The visible image among `ids` (what a context menu or double-click acts on).
    func image(in ids: Set<KuraImage.ID>) -> KuraImage? {
        visibleImages.first { ids.contains($0.id) }
    }

    /// Re-filter, then keep the selection if it still matches, else select the
    /// best match. (The TUI always jumps to the best match; keeping a still
    /// visible selection is less jumpy with the mouse.)
    private func refreshVisible() {
        visibleImages = SearchQuery(searchText).filter(images)
        if !visibleImages.contains(where: { $0.id == selection }) {
            selection = visibleImages.first?.id
        }
    }

    func focusSearch() {
        searchRequests += 1
    }

    /// Copy an image (default: the selected one) to the clipboard. Returns
    /// whether it worked. `restoring` puts the clipboard back if it fails.
    @discardableResult
    func copy(_ image: KuraImage? = nil, restoring snapshot: Pasteboard.Snapshot? = nil) async -> Bool {
        guard let client, let image = image ?? selected else { return false }
        copyGeneration += 1
        let generation = copyGeneration
        do {
            let data = try await client.file(image)
            // A newer copy started while this one downloaded: it wins.
            guard generation == copyGeneration else { return false }
            try Pasteboard.write(data, of: image)
            show("Copied \(image.name)")
            return true
        } catch {
            if let snapshot { Pasteboard.restore(snapshot) }
            if generation == copyGeneration { show("Couldn't copy: \(error.localizedDescription)") }
            return false
        }
    }

    /// Like `kura pick`: copy, then get out of the way so you can paste.
    func pick(_ image: KuraImage? = nil) async {
        if await copy(image) {
            NSApp.hide(nil)
        }
    }

    private func show(_ message: String) {
        notice = message
        noticeTask?.cancel()
        noticeTask = Task {
            try? await Task.sleep(for: .seconds(2))
            if !Task.isCancelled { notice = nil }
        }
    }

    var isStarting: Bool { client == nil && error == nil }

    /// Connect (starting the server if needed), load, then follow changes forever.
    func run() async {
        await connect()
        await follow()
    }

    /// After a failure: start over.
    func retry() async {
        error = nil
        if client == nil { await connect() } else { await load() }
    }

    func shutdown() {
        server.stop()
    }

    private func connect() async {
        let environment = ProcessInfo.processInfo.environment
        do {
            if let raw = environment["KURA_URL"], let url = URL(string: raw) {
                client = KuraClient(baseURL: url)
            } else {
                let library = environment["KURA_LIBRARY"].flatMap { $0.isEmpty ? nil : $0 }
                client = KuraClient(baseURL: try await server.start(library: library))
            }
            await load()
        } catch {
            self.error = error.localizedDescription
        }
    }

    func load() async {
        guard let client else { return }
        isLoading = true
        defer { isLoading = false }
        do {
            images = try await client.images()
            error = nil
            refreshVisible()
        } catch {
            self.error = error.localizedDescription
        }
    }

    /// Reload whenever the library changes, from any client, until cancelled.
    /// Follows `GET /events` (Server-Sent Events) and reconnects if it drops.
    /// Every event just triggers a reload: simple, and right for a small library.
    private func follow() async {
        while !Task.isCancelled {
            if let client {
                do {
                    let (bytes, response) = try await URLSession.shared.bytes(from: client.url("/events"))
                    guard (response as? HTTPURLResponse)?.statusCode == 200 else {
                        throw URLError(.badServerResponse)
                    }
                    isLive = true
                    await load()  // catch up on anything missed while disconnected
                    for try await line in bytes.lines where line.hasPrefix("event:") {
                        await load()
                    }
                } catch {
                    // Connection dropped: try again shortly.
                }
                isLive = false
            }
            try? await Task.sleep(for: .seconds(2))
        }
    }
}
