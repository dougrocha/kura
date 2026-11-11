import AppKit
import UniformTypeIdentifiers

/// Puts an image on the clipboard two ways: as image data, for apps that paste
/// pictures (Notes, Pages, Preview), and as a file, for apps that paste
/// attachments (Messages, Slack, Discord). The file keeps GIFs animated.
enum Pasteboard {
    static func copy(_ image: KuraImage, from client: KuraClient, to pasteboard: NSPasteboard = .general) async throws {
        let data = try await client.file(image)
        let type = image.mimeType.flatMap { UTType(mimeType: $0) } ?? .image

        // A copy named after the image, so the pasted attachment isn't called <hash>.png.
        let folder = FileManager.default.temporaryDirectory.appending(path: "Kura Clipboard", directoryHint: .isDirectory)
        try? FileManager.default.removeItem(at: folder)  // only the latest copy is needed
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let ext = type.preferredFilenameExtension ?? "img"
        let file = folder.appending(path: "\(fileName(image.name)).\(ext)")
        try data.write(to: file)

        let item = NSPasteboardItem()
        item.setData(data, forType: NSPasteboard.PasteboardType(type.identifier))
        item.setString(file.absoluteString, forType: .fileURL)

        pasteboard.clearContents()
        pasteboard.writeObjects([item])
    }

    /// Names can hold any character; file names can't hold `/` or `:`.
    private static func fileName(_ name: String) -> String {
        let cleaned = name.replacingOccurrences(of: "/", with: "-").replacingOccurrences(of: ":", with: "-")
        return cleaned.isEmpty ? "image" : cleaned
    }
}
