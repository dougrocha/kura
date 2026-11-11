import AppKit
import Foundation
import Testing

@testable import Kura

@MainActor
struct PasteboardTests {
    /// A tiny real GIF.
    private static let gif = Data(base64Encoded: "R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==")!

    private func image(_ name: String) -> KuraImage {
        KuraImage(
            hash: "abc", name: name, mimeType: "image/gif", createdAt: "", tags: [],
            fileUrl: "/abc.gif", thumbnailUrl: "")
    }

    /// A private pasteboard and folder, so tests never touch the real clipboard
    /// or the app's `Kura Clipboard` folder.
    private func scratch() throws -> (NSPasteboard, URL) {
        let folder = FileManager.default.temporaryDirectory.appending(path: "kura-tests-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        return (NSPasteboard(name: .init("kura-tests-\(UUID().uuidString)")), folder)
    }

    @Test func downloadsAndCopiesImageDataAndANamedFile() async throws {
        let (pasteboard, folder) = try scratch()
        defer {
            pasteboard.releaseGlobally()
            try? FileManager.default.removeItem(at: folder)
        }
        // URLSession reads file:// URLs the same way it reads the server's http:// ones.
        try Self.gif.write(to: folder.appending(path: "abc.gif"))
        let data = try await KuraClient(baseURL: folder).file(image("cat/jam"))

        try Pasteboard.write(data, of: image("cat/jam"), to: pasteboard, folder: folder.appending(path: "clip"))

        #expect(pasteboard.data(forType: .init("com.compuserve.gif")) == Self.gif)
        let fileURL = try #require(pasteboard.string(forType: .fileURL).flatMap(URL.init(string:)))
        #expect(fileURL.lastPathComponent == "cat-jam.gif")
        #expect(try Data(contentsOf: fileURL) == Self.gif)
    }

    @Test func longNamesStillMakeAFile() throws {
        let (pasteboard, folder) = try scratch()
        defer {
            pasteboard.releaseGlobally()
            try? FileManager.default.removeItem(at: folder)
        }
        let name = String(repeating: "😹", count: 200)  // a legal kura name, 400 UTF-16 units
        #expect(Pasteboard.fileName(name).utf16.count <= 200)
        try Pasteboard.write(Self.gif, of: image(name), to: pasteboard, folder: folder)
        #expect(pasteboard.string(forType: .fileURL) != nil)
    }

    @Test func restoresOnlyAJustClearedPasteboard() throws {
        let (pasteboard, folder) = try scratch()
        defer {
            pasteboard.releaseGlobally()
            try? FileManager.default.removeItem(at: folder)
        }
        pasteboard.clearContents()
        pasteboard.setString("before", forType: .string)

        let snapshot = Pasteboard.snapshot(of: pasteboard)
        pasteboard.clearContents()  // what SwiftUI may do for ⌘C
        Pasteboard.restore(snapshot, to: pasteboard)
        #expect(pasteboard.string(forType: .string) == "before")

        // Something newer was copied meanwhile: leave it alone.
        let again = Pasteboard.snapshot(of: pasteboard)
        pasteboard.clearContents()
        pasteboard.setString("newer", forType: .string)
        Pasteboard.restore(again, to: pasteboard)
        #expect(pasteboard.string(forType: .string) == "newer")
    }
}
