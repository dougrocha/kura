import Foundation
import Observation

/// What the window shows: the image list, the selection, and whether loading failed.
@MainActor
@Observable
final class LibraryModel {
    let client: KuraClient
    private(set) var images: [KuraImage] = []
    private(set) var error: String?
    private(set) var isLoading = false
    /// Connected to the server's change feed, so the list updates by itself.
    private(set) var isLive = false
    var selection: KuraImage.ID?

    init(client: KuraClient) {
        self.client = client
    }

    var selected: KuraImage? {
        images.first { $0.id == selection }
    }

    func load() async {
        isLoading = true
        defer { isLoading = false }
        do {
            images = try await client.images()
            error = nil
            if selected == nil { selection = images.first?.id }
        } catch {
            self.error = error.localizedDescription
        }
    }

    /// Reload whenever the library changes, from any client, until cancelled.
    /// Follows `GET /events` (Server-Sent Events) and reconnects if it drops.
    /// Every event just triggers a reload: simple, and right for a small library.
    func follow() async {
        while !Task.isCancelled {
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
                // Server down or connection dropped: try again shortly.
            }
            isLive = false
            try? await Task.sleep(for: .seconds(2))
        }
    }
}
