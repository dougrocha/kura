import AppKit
import Foundation
import Testing
import UniformTypeIdentifiers

@testable import Kura

@MainActor
struct AddTests {
    /// A tiny real GIF.
    private static let gif = Data(base64Encoded: "R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==")!

    private static var tiff: Data {
        NSBitmapImageRep(data: gif)!.tiffRepresentation!
    }

    private func provider(_ items: [(UTType, Data)]) -> NSItemProvider {
        let provider = NSItemProvider()
        for (type, data) in items {
            provider.registerDataRepresentation(for: type) { done in
                done(data, nil)
                return nil
            }
        }
        return provider
    }

    @Test func fileURLWinsOverEverythingElse() async {
        let file = URL(fileURLWithPath: "/tmp/cat.gif")
        let p = provider([(.fileURL, file.dataRepresentation), (.gif, Self.gif)])
        #expect(await NewImage.load(p) == .file(file))
    }

    @Test func webURLKeepsTheDraggedImageAsFallback() async {
        let web = URL(string: "https://example.com/cat.gif")!
        let p = provider([(.url, web.dataRepresentation), (.tiff, Self.tiff), (.gif, Self.gif)])
        #expect(await NewImage.load(p) == .web(web, fallback: Self.gif))
    }

    @Test func tiffOnlyIsConvertedToPNG() async throws {
        let p = provider([(.tiff, Self.tiff)])
        guard case .data(let data, _) = await NewImage.load(p) else {
            Issue.record("expected image data")
            return
        }
        #expect(data.starts(with: [0x89, 0x50, 0x4E, 0x47]))
    }

    @Test func nonImagesAreSkipped() async {
        let p = provider([(.plainText, Data("hello".utf8))])
        #expect(await NewImage.load(p) == nil)
    }

    @Test func summaries() {
        #expect(AddSummary(added: ["cat"]).message == "Added cat")
        #expect(AddSummary(added: ["a", "b"], existing: ["c"]).message == "Added 2 images · 1 already in Kura")
        #expect(AddSummary(existing: ["cat"]).message == "cat is already in Kura")
        #expect(AddSummary(failed: [("x.txt", "nope")]).message == "Couldn't add x.txt: nope")
    }

    /// Against a real server: set KURA_BIN to a built kura-server (`just mac-test` does).
    @Test(.enabled(if: ProcessInfo.processInfo.environment["KURA_BIN"] != nil))
    func addsFilesAndBytesToARealServer() async throws {
        let folder = FileManager.default.temporaryDirectory.appending(path: "kura-add-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let server = ServerProcess()
        defer {
            server.stop()
            try? FileManager.default.removeItem(at: folder)
        }
        let client = KuraClient(baseURL: try await server.start(library: folder.appending(path: "lib").path))

        let file = folder.appending(path: "cat jam.gif")
        try Self.gif.write(to: file)
        let first = try await client.add(.file(file))
        #expect(first.created)
        #expect(first.image.name == "cat-jam")  // slugged, like `kura add`
        #expect(first.image.mimeType == "image/gif")

        // Same bytes again: already there, not an error.
        let again = try await client.add(.data(Self.gif, fileName: nil))
        #expect(!again.created)
        #expect(again.image.hash == first.image.hash)

        // TIFF is converted to PNG before upload.
        let tiff = try await client.add(.data(Self.tiff, fileName: "shot.tiff"))
        #expect(tiff.created)
        #expect(tiff.image.mimeType == "image/png")

        let junk = folder.appending(path: "notes.txt")
        try Data("hi".utf8).write(to: junk)
        await #expect(throws: KuraError.self) { try await client.add(.file(junk)) }
    }
}
