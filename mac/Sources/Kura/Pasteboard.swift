import AppKit
import UniformTypeIdentifiers

/// Puts an image on the clipboard two ways: as image data, for apps that paste
/// pictures (Notes, Pages, Preview), and as a file, for apps that paste
/// attachments (Messages, Slack, Discord). The file keeps GIFs animated.
///
/// Main actor, and synchronous: the caller downloads first, then the file and
/// the clipboard change together, so overlapping copies can't mix them up.
@MainActor
enum Pasteboard {
    /// Where the named copies go. Only the latest copy is kept.
    static let defaultFolder = FileManager.default.temporaryDirectory
        .appending(path: "Kura Clipboard", directoryHint: .isDirectory)

    static func write(
        _ data: Data, of image: KuraImage,
        to pasteboard: NSPasteboard = .general, folder: URL = defaultFolder
    ) throws {
        let type = image.mimeType.flatMap { UTType(mimeType: $0) } ?? .image

        // A copy named after the image, so the pasted attachment isn't called <hash>.png.
        try? FileManager.default.removeItem(at: folder)
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

    /// A file name from an image name: no `/` or `:`, and short enough for
    /// APFS (255 UTF-16 units), since names can be 200 emoji.
    static func fileName(_ name: String) -> String {
        var cleaned = name.replacingOccurrences(of: "/", with: "-").replacingOccurrences(of: ":", with: "-")
        while cleaned.utf16.count > 200 { cleaned.removeLast() }
        return cleaned.isEmpty ? "image" : cleaned
    }

    /// What was on a pasteboard, to put back if a copy fails after the
    /// pasteboard was already cleared (SwiftUI may clear it for ⌘C).
    struct Snapshot {
        fileprivate let changeCount: Int
        fileprivate let items: [[NSPasteboard.PasteboardType: Data]]
    }

    static func snapshot(of pasteboard: NSPasteboard = .general) -> Snapshot {
        let items = (pasteboard.pasteboardItems ?? []).map { item in
            Dictionary(uniqueKeysWithValues: item.types.compactMap { type in item.data(forType: type).map { (type, $0) } })
        }
        return Snapshot(changeCount: pasteboard.changeCount, items: items)
    }

    /// Put `snapshot` back, but only if the pasteboard was just cleared since:
    /// anything newer, from this app or another, wins.
    static func restore(_ snapshot: Snapshot, to pasteboard: NSPasteboard = .general) {
        guard pasteboard.changeCount == snapshot.changeCount + 1,
            pasteboard.pasteboardItems?.isEmpty ?? true
        else { return }
        let items = snapshot.items.map { types in
            let item = NSPasteboardItem()
            for (type, data) in types { item.setData(data, forType: type) }
            return item
        }
        pasteboard.clearContents()
        pasteboard.writeObjects(items)
    }
}
