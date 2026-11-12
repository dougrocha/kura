import Foundation
import Observation

/// What the queue needs from the server.
protocol CaptionBackend: Sendable {
    func file(_ image: KuraImage) async throws -> Data
    func setCaption(_ hash: String, source: CaptionSource, text: String, engine: String?) async throws -> KuraImage
}

extension KuraClient: CaptionBackend {}

/// Reads the text of every image that needs it and stores the result.
///
/// It keeps no work list of its own: the work is derived from the library each
/// time `reconcile` runs, so images added by this app, by the CLI, or before the
/// app existed all arrive the same way, and so do engine upgrades.
@MainActor
@Observable
final class CaptionQueue {
    struct Progress: Equatable {
        var done: Int
        var total: Int
    }

    /// nil when idle. Drives "Reading text… 120 of 4,500".
    private(set) var progress: Progress?

    /// Called with each image the server returns after a write.
    @ObservationIgnored var onWrite: ((KuraImage) -> Void)?

    static let maxAttempts = 3

    @ObservationIgnored private let read: @Sendable (Data) async throws -> String
    @ObservationIgnored private let engine: String
    @ObservationIgnored private var latest: [KuraImage] = []
    @ObservationIgnored private var backend: (any CaptionBackend)?
    @ObservationIgnored private var attempts: [String: Int] = [:]
    @ObservationIgnored private var worker: Task<Void, Never>?
    /// Identifies the current pass, so a cancelled one can't clear its successor's state.
    @ObservationIgnored private var run = 0

    init(
        engine: String = Captioner.engine,
        read: @escaping @Sendable (Data) async throws -> String = { try await Captioner().read($0) }
    ) {
        self.engine = engine
        self.read = read
    }

    /// Whether an image's text should be read now.
    ///
    /// - never read: yes
    /// - read by a different engine: yes (the recognizer changed)
    /// - read by this engine, even with no text found: no (that is an answer)
    /// - typed by a person, even blank: no (their word is final)
    nonisolated static func needsReading(_ image: KuraImage, engine: String = Captioner.engine) -> Bool {
        guard let caption = image.caption else { return true }
        return caption.source == CaptionSource.ocr.rawValue && caption.engine != engine
    }

    /// Called after every load. Cheap and idempotent: it starts the worker when
    /// there is work and none is running; a running worker finds new work itself.
    func reconcile(_ images: [KuraImage], backend: (any CaptionBackend)?) {
        latest = images
        self.backend = backend
        guard backend != nil else {
            cancel()
            return
        }
        if worker == nil {
            guard !candidates(skipping: []).isEmpty else { return }
            run += 1
            let id = run
            worker = Task(priority: .utility) { await drain(id) }
        }
    }

    /// Forget past failures for this image, so a new request for it isn't refused.
    func forget(_ hash: String) {
        attempts[hash] = nil
    }

    func cancel() {
        run += 1
        worker?.cancel()
        worker = nil
        progress = nil
    }

    /// Wait for the current pass to end.
    func idle() async {
        await worker?.value
    }

    /// Newest first, which is the order the server lists them in.
    private func candidates(skipping skip: Set<String>) -> [KuraImage] {
        latest.filter {
            Self.needsReading($0, engine: engine) && attempts[$0.hash, default: 0] < Self.maxAttempts
                && !skip.contains($0.hash)
        }
    }

    private func drain(_ id: Int) async {
        var skip: Set<String> = []
        var done = 0
        defer {
            if run == id {
                progress = nil
                worker = nil
            }
        }
        while !Task.isCancelled, run == id, let backend {
            let todo = candidates(skipping: skip)
            guard let image = todo.first else { return }
            progress = Progress(done: done, total: done + todo.count)
            skip.insert(image.hash)
            done += 1
            do {
                let text = try await read(try await backend.file(image))
                let updated = try await backend.setCaption(image.hash, source: .ocr, text: text, engine: engine)
                if Self.needsReading(updated, engine: engine) { attempts[image.hash, default: 0] += 1 }
                if let index = latest.firstIndex(where: { $0.hash == updated.hash }) { latest[index] = updated }
                onWrite?(updated)
            } catch {
                if Task.isCancelled { return }
                switch EditFailure(error) {
                case .gone:
                    continue
                case .unavailable where error is URLError:
                    // The server isn't there: stop, and try again on the next reload.
                    return
                default:
                    // A failure writes nothing: "couldn't read" is not "no text".
                    attempts[image.hash, default: 0] += 1
                }
            }
        }
    }
}
