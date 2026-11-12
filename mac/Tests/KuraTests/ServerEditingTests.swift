import AppKit
import Foundation
import Testing

@testable import Kura

/// Against a real server: set KURA_BIN to a built kura-server (`just mac-test` does).
private let hasServer = ProcessInfo.processInfo.environment["KURA_BIN"] != nil

/// A distinct, tiny PNG per color, so each is its own image.
@MainActor
private func png(_ color: NSColor) -> Data {
    let image = NSImage(size: NSSize(width: 16, height: 16), flipped: false) { rect in
        color.setFill()
        rect.fill()
        return true
    }
    return NSBitmapImageRep(data: image.tiffRepresentation!)!.representation(using: .png, properties: [:])!
}

private func isInvalid(_ error: Error) -> Bool {
    if case .invalid = EditFailure(error) { return true }
    return false
}

@MainActor
private struct Fixture {
    let folder: URL
    let server = ServerProcess()
    let client: KuraClient

    init() async throws {
        folder = FileManager.default.temporaryDirectory.appending(path: "kura-edit-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        client = KuraClient(baseURL: try await server.start(library: folder.appending(path: "lib").path))
    }

    func add(_ name: String, _ color: NSColor) async throws -> KuraImage {
        try await client.add(.data(png(color), fileName: "\(name).png")).image
    }

    func stop() {
        server.stop()
        try? FileManager.default.removeItem(at: folder)
    }
}

@MainActor
@Suite(.serialized)
struct ServerEditingTests {
    @Test(.enabled(if: hasServer))
    func editsReachTheServerAndComeBackAsImages() async throws {
        let f = try await Fixture()
        defer { f.stop() }
        let cat = try await f.add("cat", .red)
        let dog = try await f.add("dog", .blue)

        let renamed = try await f.client.rename(cat.hash, to: "cat jam")
        #expect(renamed.name == "cat jam")
        do {
            _ = try await f.client.rename(dog.hash, to: "CAT JAM")
            Issue.record("expected name_taken")
        } catch {
            #expect(EditFailure(error) == .nameTaken)
        }

        // A tag with a slash, a space and a percent sign survives the round trip.
        for tag in ["a/b", "two words", "100%", "café"] {
            #expect(try await f.client.addTag(cat.hash, tag).tags.contains(tag))
        }
        #expect(try await f.client.addTag(cat.hash, "a/b").tags.filter { $0 == "a/b" }.count == 1)
        let untagged = try await f.client.removeTag(cat.hash, "a/b")
        #expect(!untagged.tags.contains("a/b"))
        #expect(untagged.tags.contains("100%"))
        #expect(try await f.client.removeTag(cat.hash, "a/b").tags == untagged.tags)  // idempotent

        do {
            _ = try await f.client.addTag(cat.hash, "no,commas")
            Issue.record("expected invalid_input")
        } catch {
            #expect(isInvalid(error))
        }

        try await f.client.delete(dog.hash)
        do {
            try await f.client.delete(dog.hash)
            Issue.record("expected not_found")
        } catch {
            #expect(EditFailure(error) == .gone)
        }
        #expect(try await f.client.images().map(\.hash) == [cat.hash])
    }

    @Test(.enabled(if: hasServer))
    func captionsAreStoredCombinedAndSearchable() async throws {
        let f = try await Fixture()
        defer { f.stop() }
        let cat = try await f.add("cat", .red)
        let dog = try await f.add("dog", .blue)
        #expect(cat.caption == nil)

        let read = try await f.client.setCaption(cat.hash, source: .ocr, text: "when the\nmoon  hits", engine: Captioner.engine)
        #expect(read.caption == KuraCaption(text: "when the moon hits", source: "ocr", engine: Captioner.engine))

        let found = try await f.client.search("moon")
        #expect(found.hits.map(\.hash) == [cat.hash])
        let snippet = try #require(found.hits.first?.snippet)
        #expect(snippet.segments.contains(Snippet.Segment(text: "moon", matched: true)))
        #expect(found.images == 2)
        #expect(found.unread == 1)

        // By name there is no snippet; the empty query is everything.
        let byName = try await f.client.search("dog")
        #expect(byName.hits.map(\.hash) == [dog.hash])
        #expect(byName.hits.first?.snippet == nil)
        #expect(try await f.client.search("").hits.count == 2)
        #expect(try await f.client.search("!moon").hits.map(\.hash) == [dog.hash])
        #expect(try await f.client.search("#nothing 'x+y&z").hits.isEmpty)

        // A typed caption wins; clearing it falls back to what was read.
        let typed = try await f.client.setCaption(cat.hash, source: .manual, text: "stonks")
        #expect(typed.caption?.source == "manual")
        #expect(try await f.client.search("moon").hits.isEmpty)
        #expect(try await f.client.search("stonks").hits.map(\.hash) == [cat.hash])
        let fallback = try await f.client.clearCaption(cat.hash, source: .manual)
        #expect(fallback.caption?.text == "when the moon hits")
        let unread = try await f.client.clearCaption(cat.hash, source: .ocr)
        #expect(unread.caption == nil)

        let blank = try await f.client.setCaption(dog.hash, source: .ocr, text: "  ", engine: Captioner.engine)
        #expect(blank.caption?.text == "")

        do {
            _ = try await f.client.setCaption(dog.hash, source: .ocr, text: "x", engine: nil)
            Issue.record("expected invalid_input")
        } catch {
            #expect(isInvalid(error))
        }
    }

    @Test(.enabled(if: hasServer))
    func theQueueReadsTheLibraryWithTheRealRecognizer() async throws {
        let f = try await Fixture()
        defer { f.stop() }
        let a = try await f.add("a", .red)
        let b = try await f.add("b", .green)
        let queue = CaptionQueue()
        queue.reconcile(try await f.client.images(), backend: f.client)
        await queue.idle()
        let images = try await f.client.images()
        #expect(Set(images.map(\.hash)) == [a.hash, b.hash])
        for image in images {
            #expect(image.caption == KuraCaption(text: "", source: "ocr", engine: Captioner.engine))
        }
        queue.reconcile(images, backend: f.client)
        #expect(queue.progress == nil)
    }

    @Test(.enabled(if: hasServer))
    func theModelSearchesEditsAndDeletesThroughTheServer() async throws {
        let f = try await Fixture()
        defer { f.stop() }
        let one = try await f.add("one", .red)
        let two = try await f.add("two", .green)
        let three = try await f.add("three", .blue)
        _ = try await f.client.setCaption(two.hash, source: .manual, text: "when the moon hits")

        setenv("KURA_URL", f.client.baseURL.absoluteString, 1)
        defer { unsetenv("KURA_URL") }
        let model = LibraryModel()
        let running = Task { await model.run() }
        defer { running.cancel() }

        func eventually(_ what: String, _ condition: () -> Bool) async throws {
            for _ in 0..<400 {
                if condition() { return }
                try await Task.sleep(for: .milliseconds(25))
            }
            Issue.record("timed out waiting for \(what)")
        }

        try await eventually("the library") { model.images.count == 3 }
        // The queue reads one and three; two has typed text.
        try await eventually("text to be read") {
            model.images.allSatisfy { $0.caption != nil } && model.captions.progress == nil
        }

        // Search: the server ranks, the model shows images in that order with the snippet.
        model.searchText = "moon"
        try await eventually("the moon search") { model.visibleImages.map(\.hash) == [two.hash] }
        #expect(model.snippets[two.hash]?.segments.contains(Snippet.Segment(text: "moon", matched: true)) == true)
        #expect(model.selection == two.hash)

        model.searchText = "thr"
        try await eventually("the name search") { model.visibleImages.map(\.hash) == [three.hash] }
        #expect(model.snippets.isEmpty)
        #expect(model.selection == three.hash)

        // Clearing the search is local: everything, no snippets.
        model.searchText = ""
        #expect(model.visibleImages.count == 3)

        // Rename keeps the selection (by hash) and patches the image in place.
        model.selection = one.hash
        let taken = await model.rename(model.images.first { $0.hash == one.hash }!, to: "TWO")
        #expect(taken == .nameTaken)
        #expect(model.error == nil)
        let ok = await model.rename(model.images.first { $0.hash == one.hash }!, to: "uno")
        #expect(ok == nil)
        #expect(model.images.first { $0.hash == one.hash }?.name == "uno")
        #expect(model.selection == one.hash)

        // Delete moves the selection to the neighbor first and drops the image.
        let before = model.visibleImages.map(\.hash)
        let position = try #require(before.firstIndex(of: one.hash))
        let neighbor = before.indices.contains(position + 1) ? before[position + 1] : before[position - 1]
        await model.delete(model.images.first { $0.hash == one.hash }!)
        #expect(model.selection == neighbor)
        #expect(!model.images.contains { $0.hash == one.hash })
        #expect(try await f.client.images().count == 2)

        // Deleting what is already gone is fine.
        await model.delete(KuraImage(hash: one.hash, name: "uno", mimeType: nil, createdAt: "", tags: [], fileUrl: "", thumbnailUrl: ""))
        #expect(model.images.count == 2)
    }
}
