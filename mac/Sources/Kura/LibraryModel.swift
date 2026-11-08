import Foundation
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

    @ObservationIgnored private let server = ServerProcess()

    init() {
        server.onUnexpectedExit = { [weak self] message in
            self?.client = nil
            self?.isLive = false
            self?.error = message
        }
    }

    var selected: KuraImage? {
        images.first { $0.id == selection }
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
            if selected == nil { selection = images.first?.id }
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
