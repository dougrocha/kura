import AppKit
import Foundation
import ImageIO
import Testing

@testable import Kura

private func image(_ hash: String, caption: KuraCaption? = nil) -> KuraImage {
    KuraImage(
        hash: hash, name: hash, mimeType: "image/png", createdAt: "", tags: [], fileUrl: "/\(hash)",
        thumbnailUrl: "", caption: caption)
}

private func ocr(_ text: String = "x", engine: String = Captioner.engine) -> KuraCaption {
    KuraCaption(text: text, source: "ocr", engine: engine)
}

private func manual(_ text: String = "x") -> KuraCaption {
    KuraCaption(text: text, source: "manual", engine: nil)
}

struct NeedsReadingTests {
    @Test func truthTable() {
        let engine = "vision:2"
        func needs(_ caption: KuraCaption?) -> Bool {
            CaptionQueue.needsReading(image("a", caption: caption), engine: engine)
        }
        #expect(needs(nil))
        #expect(needs(ocr("text", engine: "vision:0")))
        #expect(needs(ocr("", engine: "vision:0")))
        #expect(!needs(ocr("text", engine: engine)))
        #expect(!needs(ocr("", engine: engine)))
        #expect(!needs(manual("text")))
        #expect(!needs(manual("")))
    }

    @Test func theStoredEngineIsTheContractsName() {
        #expect(Captioner.engine == "vision:2")
    }
}

/// A server that records what the queue writes.
private actor FakeBackend: CaptionBackend {
    struct Write: Equatable {
        let hash: String
        let source: CaptionSource
        let text: String
        let engine: String?
    }

    private(set) var writes: [Write] = []
    private(set) var fileCalls: [String] = []
    var gone: Set<String> = []
    var unreachable: Set<String> = []
    /// Answer with an image that still has no caption, like a broken server.
    var forgetful = false

    func set(gone: Set<String> = [], unreachable: Set<String> = [], forgetful: Bool = false) {
        self.gone = gone
        self.unreachable = unreachable
        self.forgetful = forgetful
    }

    func file(_ image: KuraImage) async throws -> Data {
        fileCalls.append(image.hash)
        if unreachable.contains(image.hash) { throw URLError(.cannotConnectToHost) }
        if gone.contains(image.hash) { throw KuraError(code: "not_found", message: "no such image") }
        return Data(image.hash.utf8)
    }

    func setCaption(_ hash: String, source: CaptionSource, text: String, engine: String?) async throws -> KuraImage {
        writes.append(Write(hash: hash, source: source, text: text, engine: engine))
        return image(hash, caption: forgetful ? nil : KuraCaption(text: text, source: source.rawValue, engine: engine))
    }
}

private struct Unreadable: Error {}

@MainActor
struct CaptionQueueTests {
    private func queue(failing: Set<String> = []) -> CaptionQueue {
        CaptionQueue(read: { data in
            let hash = String(decoding: data, as: UTF8.self)
            if failing.contains(hash) { throw Unreadable() }
            return "text of \(hash)"
        })
    }

    @Test func readsWhatNeedsReadingNewestFirstAndSkipsTheRest() async {
        let backend = FakeBackend()
        let queue = queue()
        var written: [String] = []
        queue.onWrite = { written.append($0.hash) }
        queue.reconcile(
            [
                image("new"), image("done", caption: ocr()), image("old-engine", caption: ocr(engine: "vision:0")),
                image("typed", caption: manual()), image("oldest"),
            ], backend: backend)
        await queue.idle()
        #expect(await backend.writes.map(\.hash) == ["new", "old-engine", "oldest"])
        #expect(await backend.writes.allSatisfy { $0.source == .ocr && $0.engine == Captioner.engine })
        #expect(await backend.writes.first?.text == "text of new")
        #expect(written == ["new", "old-engine", "oldest"])
        #expect(queue.progress == nil)
    }

    @Test func aFailureWritesNothingAndIsRetriedUpToThreeTimes() async {
        let backend = FakeBackend()
        let queue = queue(failing: ["bad"])
        var images = [image("bad"), image("good")]
        queue.onWrite = { written in images = images.map { $0.hash == written.hash ? written : $0 } }
        for _ in 0..<5 {
            queue.reconcile(images, backend: backend)
            await queue.idle()
        }
        #expect(await backend.fileCalls.filter { $0 == "bad" }.count == 3)
        #expect(await backend.writes.map(\.hash) == ["good"])
    }

    @Test func forgettingAnImageAllowsAFreshTry() async {
        let backend = FakeBackend()
        let queue = queue(failing: ["bad"])
        for _ in 0..<4 {
            queue.reconcile([image("bad")], backend: backend)
            await queue.idle()
        }
        queue.forget("bad")
        queue.reconcile([image("bad")], backend: backend)
        await queue.idle()
        #expect(await backend.fileCalls.count == 4)
    }

    @Test func anImageDeletedMeanwhileIsSkippedWithoutCountingAgainstIt() async {
        let backend = FakeBackend()
        await backend.set(gone: ["a"])
        let queue = queue()
        queue.reconcile([image("a"), image("b")], backend: backend)
        await queue.idle()
        #expect(await backend.writes.map(\.hash) == ["b"])
    }

    @Test func anUnreachableServerStopsThePassAndTheNextReloadResumes() async {
        let backend = FakeBackend()
        await backend.set(unreachable: ["a"])
        let queue = queue()
        let images = [image("a"), image("b")]
        queue.reconcile(images, backend: backend)
        await queue.idle()
        #expect(await backend.writes.isEmpty)
        #expect(await backend.fileCalls == ["a"])
        await backend.set()
        queue.reconcile(images, backend: backend)
        await queue.idle()
        #expect(await backend.writes.map(\.hash) == ["a", "b"])
    }

    @Test func aServerThatForgetsWhatWasWrittenDoesNotLoopForever() async {
        let backend = FakeBackend()
        await backend.set(forgetful: true)
        let queue = queue()
        for _ in 0..<6 {
            queue.reconcile([image("a")], backend: backend)
            await queue.idle()
        }
        #expect(await backend.writes.count == CaptionQueue.maxAttempts)
    }

    @Test func noClientCancelsTheWork() async {
        let queue = queue()
        queue.reconcile([image("a")], backend: nil)
        #expect(queue.progress == nil)
        await queue.idle()
    }
}

@MainActor
struct CoalescingTests {
    private final class Counter {
        var runs = 0
    }

    @Test func aBurstOfPokesRunsOnce() async {
        let counter = Counter()
        let coalescer = Coalescer(interval: .milliseconds(20)) { counter.runs += 1 }
        for _ in 0..<10 { coalescer.poke() }
        await coalescer.idle()
        #expect(counter.runs == 1)
    }

    @Test func aPokeWhileItRunsRunsAgain() async throws {
        let counter = Counter()
        let gate = Gate()
        let coalescer = Coalescer(interval: .milliseconds(5)) {
            counter.runs += 1
            if counter.runs == 1 { await gate.wait() }
        }
        coalescer.poke()
        while await !gate.waiting { try await Task.sleep(for: .milliseconds(5)) }
        coalescer.poke()
        coalescer.poke()
        await gate.open()
        await coalescer.idle()
        #expect(counter.runs == 2)
    }

    @Test func pokingAgainAfterItSettledRunsAgain() async {
        let counter = Counter()
        let coalescer = Coalescer(interval: .milliseconds(10)) { counter.runs += 1 }
        coalescer.poke()
        await coalescer.idle()
        coalescer.poke()
        await coalescer.idle()
        #expect(counter.runs == 2)
    }
}

private actor Gate {
    private var continuation: CheckedContinuation<Void, Never>?
    private(set) var waiting = false

    func wait() async {
        await withCheckedContinuation { continuation in
            self.continuation = continuation
            waiting = true
        }
    }

    func open() {
        continuation?.resume()
        continuation = nil
    }
}

@MainActor
struct SearchRunnerTests {
    private func response(_ hashes: [String]) -> SearchResponse {
        SearchResponse(seq: 1, hits: hashes.map { SearchHit(hash: $0, snippet: nil) }, images: hashes.count, unread: 0)
    }

    @Test func aResponseAfterANewerSearchStartedIsDropped() async throws {
        var delivered: [[String]] = []
        let runner = SearchRunner { result in
            if case .success(let response) = result { delivered.append(response.hits.map(\.hash)) }
        }
        let gate = Gate()
        let old = response(["old"])
        let new = response(["new"])
        // Ignores cancellation, like a request that was already on the wire.
        runner.start("a") { _ in
            await gate.wait()
            return old
        }
        while await !gate.waiting { try await Task.sleep(for: .milliseconds(5)) }
        runner.start("ab") { _ in new }
        await runner.settled()
        await gate.open()
        try await Task.sleep(for: .milliseconds(50))
        #expect(delivered == [["new"]])
    }

    @Test func cancelDropsTheSearchInFlight() async throws {
        var delivered = 0
        let runner = SearchRunner { _ in delivered += 1 }
        let gate = Gate()
        let late = response(["a"])
        runner.start("a") { _ in
            await gate.wait()
            return late
        }
        while await !gate.waiting { try await Task.sleep(for: .milliseconds(5)) }
        runner.cancel()
        await gate.open()
        try await Task.sleep(for: .milliseconds(50))
        #expect(delivered == 0)
    }

    @Test func failuresAreDelivered() async {
        var failed: Error?
        let runner = SearchRunner { result in
            if case .failure(let error) = result { failed = error }
        }
        runner.start("a") { _ in throw KuraError(code: "no_route", message: "no route") }
        await runner.settled()
        #expect((failed as? KuraError)?.code == "no_route")
    }
}

@MainActor
struct SelectionTests {
    private let visible = [image("a"), image("b"), image("c")]

    @Test func theSelectionStaysWhileItIsListed() {
        #expect(LibraryModel.selection(keeping: "b", in: visible) == "b")
    }

    @Test func aMissingSelectionMovesToTheFirstHit() {
        #expect(LibraryModel.selection(keeping: "gone", in: visible) == "a")
        #expect(LibraryModel.selection(keeping: nil, in: visible) == "a")
        #expect(LibraryModel.selection(keeping: "a", in: []) == nil)
    }

    @Test func deletingMovesToTheNextThenThePrevious() {
        #expect(LibraryModel.neighbor(of: "a", in: visible) == "b")
        #expect(LibraryModel.neighbor(of: "b", in: visible) == "c")
        #expect(LibraryModel.neighbor(of: "c", in: visible) == "b")
        #expect(LibraryModel.neighbor(of: "a", in: [image("a")]) == nil)
        #expect(LibraryModel.neighbor(of: "z", in: visible) == nil)
    }
}

struct CaptionerTests {
    private func line(_ text: String, x: Double, y: Double) -> Captioner.Line {
        Captioner.Line(text: text, box: CGRect(x: x, y: y, width: 0.2, height: 0.1))
    }

    @Test func linesReadTopToBottomThenLeftToRight() {
        let lines = [
            line("C", x: 0.3, y: 0.5), line("B", x: 0.6, y: 0.81), line("D", x: 0.3, y: 0.1), line("A", x: 0.1, y: 0.8),
        ]
        #expect(Captioner.ordered(lines) == ["A", "B", "C", "D"])
        #expect(Captioner.ordered([]) == [])
    }

    @MainActor
    private static func png(width: Int, height: Int, text: String?) -> Data {
        let image = NSImage(size: NSSize(width: width, height: height), flipped: false) { rect in
            NSColor.white.setFill()
            rect.fill()
            if let text {
                let attributes: [NSAttributedString.Key: Any] = [
                    .font: NSFont.systemFont(ofSize: 110, weight: .heavy), .foregroundColor: NSColor.black,
                ]
                NSAttributedString(string: text, attributes: attributes).draw(at: NSPoint(x: 30, y: 40))
            }
            return true
        }
        let tiff = image.tiffRepresentation!
        return NSBitmapImageRep(data: tiff)!.representation(using: .png, properties: [:])!
    }

    @Test @MainActor func readsTextDrawnInAnImage() async throws {
        let text = try await Captioner().read(Self.png(width: 900, height: 220, text: "HELLO MOON"))
        #expect(text.lowercased().contains("hello"))
        #expect(text.lowercased().contains("moon"))
    }

    @Test @MainActor func anImageWithoutTextIsAnEmptyAnswer() async throws {
        #expect(try await Captioner().read(Self.png(width: 400, height: 200, text: nil)) == "")
    }

    /// A GIF of `frames`, each blank or with its text drawn.
    @MainActor static func gif(_ frames: [String?]) -> Data {
        let data = NSMutableData()
        let destination = CGImageDestinationCreateWithData(data, "com.compuserve.gif" as CFString, frames.count, nil)!
        for text in frames {
            let png = png(width: 900, height: 220, text: text)
            let image = CGImageSourceCreateImageAtIndex(CGImageSourceCreateWithData(png as CFData, nil)!, 0, nil)!
            CGImageDestinationAddImage(destination, image, nil)
        }
        CGImageDestinationFinalize(destination)
        return data as Data
    }

    @Test @MainActor func animationsAreReadAtTheirMiddleAndLastFramesToo() async throws {
        let text = try await Captioner().read(Self.gif([nil, "HELLO", "MOON"]))
        #expect(text.lowercased().contains("hello"))
        #expect(text.lowercased().contains("moon"))
    }

    @Test @MainActor func textThatStaysOnScreenIsReadOnce() async throws {
        let text = try await Captioner().read(Self.gif(["MOON", "MOON", "MOON"]))
        #expect(text.lowercased().components(separatedBy: "moon").count == 2)
    }

    @Test func bytesThatAreNotAnImageFailWithoutAnAnswer() async {
        await #expect(throws: Captioner.Failure.self) { try await Captioner().read(Data("nope".utf8)) }
    }

    @Test @MainActor func bigImagesAreScaledDownButSmallOnesAreNot() {
        let big = Captioner.frames(Self.png(width: 3200, height: 100, text: nil)).first
        #expect(big.map { max($0.width, $0.height) } == 2560)
        let small = Captioner.frames(Self.png(width: 300, height: 100, text: nil)).first
        #expect(small?.width == 300)
    }
}
