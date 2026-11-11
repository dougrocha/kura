import AppKit
import Foundation
import Testing

@testable import Kura

struct PasteboardTests {
    /// A tiny real GIF, served from a folder: URLSession reads file:// URLs the
    /// same way it reads the server's http:// ones.
    private static let gif = Data(base64Encoded: "R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==")!

    @Test func copiesImageDataAndANamedFile() async throws {
        let folder = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        try Self.gif.write(to: folder.appending(path: "abc.gif"))

        let image = KuraImage(
            hash: "abc", name: "cat/jam", mimeType: "image/gif", createdAt: "", tags: [],
            fileUrl: "/abc.gif", thumbnailUrl: "")
        let pasteboard = NSPasteboard(name: .init("kura-tests-\(UUID().uuidString)"))
        defer { pasteboard.releaseGlobally() }

        try await Pasteboard.copy(image, from: KuraClient(baseURL: folder), to: pasteboard)

        #expect(pasteboard.data(forType: .init("com.compuserve.gif")) == Self.gif)
        let fileURL = try #require(pasteboard.string(forType: .fileURL).flatMap(URL.init(string:)))
        #expect(fileURL.lastPathComponent == "cat-jam.gif")
        #expect(try Data(contentsOf: fileURL) == Self.gif)
    }
}
