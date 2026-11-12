import AppKit
import Observation

/// The launcher's main page: your memes in a grid. Empty search shows the
/// ones you use most (then the newest); typing searches names, tags and the
/// words in the images, ranked by the server.
@MainActor
@Observable
final class MemesPage: LauncherPage {
    static let limit = 60

    let title = "Memes"
    let placeholder = "Find a meme by name, tag or the words in it"
    let layout = LauncherLayout.grid(columns: 5)
    var emptyTitle: String { query.isEmpty ? "No Memes Yet" : "No Matches" }

    var query = "" {
        didSet { refresh() }
    }
    /// The search's ranked hashes; nil while the search is empty. Hashes, not
    /// images, so renames and deletes show up while the launcher is open.
    private var hits: [String]?
    private var snippets: [String: Snippet] = [:]

    @ObservationIgnored let library: LibraryModel
    @ObservationIgnored private lazy var searches = SearchRunner { [weak self] result in
        guard let self, case .success(let response) = result else { return }
        hits = response.hits.map(\.hash)
        snippets = response.snippets
    }

    init(library: LibraryModel) {
        self.library = library
    }

    var items: [LauncherItem] {
        let images = hits.map { Array($0.lazy.compactMap(library.image(hash:)).prefix(Self.limit)) }
            ?? Array(Usage.ordered(library.images, by: library.usage.entries).prefix(Self.limit))
        return images.map { image in
            LauncherItem(
                id: image.hash, title: image.name, subtitle: image.subtitle(snippets[image.id]),
                image: library.client?.thumbnailURL(image, size: "grid"))
        }
    }

    func reset() {
        query = ""
    }

    private func refresh() {
        let query = query.trimmingCharacters(in: .whitespacesAndNewlines)
        if query.isEmpty {
            searches.cancel()
            hits = nil
            snippets = [:]
        } else if let client = library.client {
            searches.start(query) { try await client.search($0) }
        }
    }

    func actions(for item: LauncherItem) -> [LauncherAction] {
        guard let image = library.image(hash: item.id) else { return [] }
        return [
            LauncherAction(title: "Paste", systemImage: "doc.on.clipboard") {
                try await self.copy(image)
                return .close(then: { Paster.paste() })
            },
            LauncherAction(title: "Copy", systemImage: "doc.on.doc", shortcut: .command("c")) {
                try await self.copy(image)
                return .close()
            },
            LauncherAction(title: "Show in Kura", systemImage: "macwindow", shortcut: .command("o")) {
                self.library.searchText = ""
                self.library.selection = image.id
                NSApp.activate()
                NSApp.windows.first { $0.canBecomeMain }?.makeKeyAndOrderFront(nil)
                return .close()
            },
        ]
    }

    private func copy(_ image: KuraImage) async throws {
        guard await library.copy(image) else {
            throw KuraError(code: "copy_failed", message: "Couldn't copy \(image.name)")
        }
    }
}
