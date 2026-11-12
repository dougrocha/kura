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
    private(set) var images: [KuraImage] = [] {
        didSet { index = Dictionary(images.enumerated().map { ($1.hash, $0) }, uniquingKeysWith: { a, _ in a }) }
    }
    private(set) var error: String?
    private(set) var isLoading = false
    /// Connected to the server's change feed, so the list updates by itself.
    private(set) var isLive = false
    var selection: KuraImage.ID?
    /// The sidebar's search. The server ranks it, with the TUI's rules.
    var searchText = "" {
        didSet { refreshVisible() }
    }
    /// The images the sidebar shows: all of them, or the search's matches, best first.
    private(set) var visibleImages: [KuraImage] = []
    /// For hits where the image's text matched: the part of it to show.
    private(set) var snippets: [KuraImage.ID: Snippet] = [:]
    /// How many images have no text read yet, as the last search saw it.
    private(set) var unreadCount = 0
    /// Why the last search failed (for example, a server without `/search`).
    private(set) var searchError: String?
    /// True while the search field has focus, so ⌘E and ⌘⌫ stay the text field's.
    var isSearching = false
    /// Bumped by Find (⌘F) and `/`; the view focuses the search field.
    private(set) var searchRequests = 0
    /// Bumped by File → Add Images (⌘O); the view shows the file picker.
    private(set) var addRequests = 0
    /// Bumped by Image → Rename (⌘E) and Delete (⌘⌫); the view acts on the selection.
    private(set) var renameRequests = 0
    private(set) var deleteRequests = 0
    /// Reads the text in images (Apple Vision) and stores it on the server.
    let captions = CaptionQueue()
    /// Images still uploading or importing, for the subtitle.
    private(set) var pendingAdds = 0
    /// Shown briefly after copying ("Copied cat jam").
    private(set) var notice: String?
    /// The shared config file, through the server. nil until connected, or
    /// when talking to a server the app didn't start (`KURA_URL`).
    private(set) var config: KuraConfig?
    /// Why the last Settings change failed.
    private(set) var configError: String?
    /// When the app's own server started, for Settings.
    private(set) var serverStartedAt: Date?

    @ObservationIgnored private let server = ServerProcess()
    /// Bumped by every copy, so a slow download can't overwrite a newer copy.
    @ObservationIgnored private var copyGeneration = 0
    @ObservationIgnored private var noticeTask: Task<Void, Never>?
    /// The ranked hashes of the last search; nil shows every image.
    @ObservationIgnored private var hitOrder: [String]?
    @ObservationIgnored private var index: [String: Int] = [:]
    @ObservationIgnored private lazy var searches = SearchRunner { [weak self] result in
        self?.finishSearch(result)
    }
    /// Change events come in bursts (one per image while text is being read).
    @ObservationIgnored private lazy var reloader = Coalescer { [weak self] in
        await self?.load()
    }

    init() {
        captions.onWrite = { [weak self] image in self?.patch(image, search: false) }
        server.onUnexpectedExit = { [weak self] message in
            self?.client = nil
            self?.serverStartedAt = nil
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

    /// Show the right images for the search box. An empty search shows everything
    /// without asking the server. Otherwise the previous results stay up until the
    /// new ones arrive, so the list doesn't flash empty between keystrokes.
    private func refreshVisible() {
        let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        if query.isEmpty {
            searches.cancel()
            hitOrder = nil
            snippets = [:]
            unreadCount = 0
            searchError = nil
        } else if let client {
            searches.start(query) { try await client.search($0) }
        }
        rebuildVisible()
    }

    private func finishSearch(_ result: Result<SearchResponse, Error>) {
        switch result {
        case .success(let response):
            hitOrder = response.hits.map(\.hash)
            snippets = Dictionary(response.hits.compactMap { hit in hit.snippet.map { (hit.hash, $0) } }) { first, _ in first }
            unreadCount = response.unread
            searchError = nil
            rebuildVisible()
        case .failure(let error):
            searchError = (error as? KuraError)?.code == "no_route"
                ? "This server is too old to search. Update Kura." : error.localizedDescription
        }
    }

    /// Map the hit order to images (a hash this app doesn't have yet is skipped;
    /// the reload that brings it re-runs the search), and keep the selection.
    private func rebuildVisible() {
        if let hitOrder {
            visibleImages = hitOrder.compactMap { index[$0].map { images[$0] } }
        } else {
            visibleImages = images
        }
        selection = Self.selection(keeping: selection, in: visibleImages)
    }

    /// The selection stays if it is still listed (it follows the image through
    /// renames and re-ranking), else it moves to the first one.
    nonisolated static func selection(keeping id: KuraImage.ID?, in visible: [KuraImage]) -> KuraImage.ID? {
        if let id, visible.contains(where: { $0.id == id }) { return id }
        return visible.first?.id
    }

    /// Where the selection goes when `id` is deleted: the next one, else the previous.
    nonisolated static func neighbor(of id: KuraImage.ID, in visible: [KuraImage]) -> KuraImage.ID? {
        guard let position = visible.firstIndex(where: { $0.id == id }) else { return nil }
        if visible.indices.contains(position + 1) { return visible[position + 1].id }
        return position > 0 ? visible[position - 1].id : nil
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

    func requestAdd() {
        addRequests += 1
    }

    func requestRename() {
        renameRequests += 1
    }

    func requestDelete() {
        deleteRequests += 1
    }

    // MARK: Editing
    //
    // Each edit returns nil when it worked and the failure otherwise, for the
    // sheet or bar that asked to show. None of them touches `error`, which
    // replaces the whole detail pane. Only "it's gone" is handled here: say so
    // and reload.

    func rename(_ image: KuraImage, to name: String) async -> EditFailure? {
        await edit { try await $0.rename(image.hash, to: name) }
    }

    func addTag(_ image: KuraImage, _ tag: String) async -> EditFailure? {
        await edit { try await $0.addTag(image.hash, tag) }
    }

    func removeTag(_ image: KuraImage, _ tag: String) async -> EditFailure? {
        await edit { try await $0.removeTag(image.hash, tag) }
    }

    /// Set the text for an image by hand. Blank means "this image has no text".
    func setCaption(_ image: KuraImage, to text: String) async -> EditFailure? {
        await edit { try await $0.setCaption(image.hash, source: .manual, text: text) }
    }

    /// Drop the typed text, so the image falls back to what was read.
    func useReadText(_ image: KuraImage) async -> EditFailure? {
        let failure = await edit { try await $0.clearCaption(image.hash, source: .manual) }
        captions.forget(image.hash)
        captions.reconcile(images, backend: client)
        return failure
    }

    /// Forget what was read, so the queue reads the image again.
    func readTextAgain(_ image: KuraImage) async -> EditFailure? {
        captions.forget(image.hash)
        let failure = await edit { try await $0.clearCaption(image.hash, source: .ocr) }
        captions.reconcile(images, backend: client)
        return failure
    }

    private func edit(_ operation: (KuraClient) async throws -> KuraImage) async -> EditFailure? {
        guard let client else { return .unavailable("Kura isn't connected.") }
        do {
            patch(try await operation(client))
            return nil
        } catch {
            let failure = EditFailure(error)
            if failure == .gone { await imageGone() }
            return failure
        }
    }

    private func imageGone() async {
        show("That image was already deleted")
        await load()
    }

    /// Put an updated image in place. `search` re-ranks (the new name or text may
    /// match differently); the text reader skips it, since a reload follows.
    private func patch(_ image: KuraImage, search: Bool = true) {
        guard let position = index[image.hash] else { return }
        images[position] = image
        if search { refreshVisible() } else { rebuildVisible() }
    }

    /// Delete after the user confirmed. The selection moves to the neighbor first,
    /// so the detail pane never shows a vanished image. A 404 means it is already
    /// gone, which is what was asked for.
    func delete(_ image: KuraImage) async {
        guard let client else { return }
        let wasSelected = selection == image.id
        if wasSelected { selection = Self.neighbor(of: image.id, in: visibleImages) }
        do {
            try await client.delete(image.hash)
            show("Deleted \(image.name)")
        } catch {
            let failure = EditFailure(error)
            guard failure == .gone else {
                if wasSelected { selection = image.id }
                show("Couldn't delete \(image.name): \(failure.message())")
                return
            }
            show("\(image.name) was already deleted")
        }
        images.removeAll { $0.hash == image.hash }
        rebuildVisible()
    }

    /// Return in the search field: the best match. Waits for a search still in flight.
    func pickBestMatch() async {
        await searches.settled()
        if let first = visibleImages.first { selection = first.id }
        await pick()
    }

    /// Add images (`kura add`) one at a time, then select the last one added.
    /// Adding bytes the library already has selects that image instead.
    func add(_ items: [NewImage]) async {
        guard let client, !items.isEmpty else { return }
        pendingAdds += items.count
        var summary = AddSummary()
        var last: KuraImage?
        for item in items {
            do {
                let added = try await client.add(item)
                if added.created {
                    summary.added.append(added.image.name)
                } else {
                    summary.existing.append(added.image.name)
                }
                last = added.image
            } catch {
                summary.failed.append((item.label, error.localizedDescription))
            }
            pendingAdds -= 1
        }
        await load()
        await searches.settled()
        if let last {
            // A search that hides the new image would just look like nothing happened.
            if !visibleImages.contains(where: { $0.id == last.id }) { searchText = "" }
            selection = last.id
        }
        show(summary.message)
    }

    /// Like `kura pick`: copy, then get out of the way so you can paste.
    func pick(_ image: KuraImage? = nil) async {
        if await copy(image), config?.closeAfterCopy ?? true {
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
                serverStartedAt = .now
            }
            await load()
            await refreshConfig()
        } catch {
            self.error = error.localizedDescription
        }
    }

    /// Re-read the config file: Settings calls this when it appears, and when
    /// Kura comes back to the front after a `kura config set` in a terminal.
    func refreshConfig() async {
        guard let client, serverStartedAt != nil else { return }
        config = try? await client.config()
    }

    /// Change one setting in the shared config file (nil resets it). A new
    /// library folder takes a new server, so that restarts it.
    func setConfig(_ key: String, to value: String?) async {
        guard let client else { return }
        do {
            config = try await client.setConfig(key, to: value)
            configError = nil
            if key == "library" { await restartServer() }
        } catch {
            configError = error.localizedDescription
        }
    }

    /// Stop the app's server and start a fresh one (Settings → Restart).
    func restartServer() async {
        guard serverStartedAt != nil || error != nil else { return }
        server.stop()
        client = nil
        serverStartedAt = nil
        isLive = false
        error = nil
        images = []
        captions.cancel()
        refreshVisible()
        await connect()
    }

    func load() async {
        guard let client else { return }
        isLoading = true
        defer { isLoading = false }
        do {
            images = try await client.images()
            error = nil
            refreshVisible()
            captions.reconcile(images, backend: client)
        } catch {
            self.error = error.localizedDescription
        }
    }

    /// Reload whenever the library changes, from any client, until cancelled.
    /// Follows `GET /events` (Server-Sent Events) and reconnects if it drops.
    /// Every event just triggers a reload, at most one per 150 ms.
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
                        reloader.poke()
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
