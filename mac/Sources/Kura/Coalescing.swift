import Foundation

/// Runs an action at most once per `interval`, however often it is poked, and
/// again if it was poked while it ran. An OCR backfill makes one change event
/// per image; this turns a burst of them into one reload.
@MainActor
final class Coalescer {
    private let interval: Duration
    private let action: @MainActor () async -> Void
    private var task: Task<Void, Never>?
    private var dirty = false

    init(interval: Duration = .milliseconds(150), action: @escaping @MainActor () async -> Void) {
        self.interval = interval
        self.action = action
    }

    func poke() {
        dirty = true
        guard task == nil else { return }
        task = Task { [weak self] in
            while let self, self.dirty {
                try? await Task.sleep(for: self.interval)
                self.dirty = false
                await self.action()
            }
            self?.task = nil
        }
    }

    /// Wait until nothing is pending.
    func idle() async {
        await task?.value
    }
}

/// Runs the latest search only: starting one cancels the one in flight, and a
/// response that arrives after a newer search started is dropped.
@MainActor
final class SearchRunner {
    typealias Search = @Sendable (String) async throws -> SearchResponse

    private let deliver: @MainActor (Result<SearchResponse, Error>) -> Void
    private var generation = 0
    private var task: Task<Void, Never>?

    init(deliver: @escaping @MainActor (Result<SearchResponse, Error>) -> Void) {
        self.deliver = deliver
    }

    func start(_ query: String, search: @escaping Search) {
        task?.cancel()
        generation += 1
        let mine = generation
        task = Task { [weak self] in
            let result: Result<SearchResponse, Error>
            do {
                result = .success(try await search(query))
            } catch {
                if error is CancellationError || (error as? URLError)?.code == .cancelled { return }
                result = .failure(error)
            }
            guard let self, mine == self.generation, !Task.isCancelled else { return }
            self.deliver(result)
        }
    }

    func cancel() {
        task?.cancel()
        task = nil
        generation += 1
    }

    /// Wait for the search in flight, if any.
    func settled() async {
        await task?.value
    }
}
