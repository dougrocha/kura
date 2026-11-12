import AppKit
import UniformTypeIdentifiers

/// Something to add to the library: a file, a web address, or image bytes
/// (a paste or a drag from another app).
enum NewImage: Sendable, Equatable {
    case file(URL)
    /// The server fetches it, so a GIF stays the original GIF. `fallback` is
    /// the image the drag also carried, for when the URL isn't the image
    /// itself (an image wrapped in a link) or isn't public.
    case web(URL, fallback: Data?)
    case data(Data, fileName: String?)

    /// What to call it in a message ("Couldn't add <label>").
    var label: String {
        switch self {
        case .file(let url): url.lastPathComponent
        case .web(let url, _): url.lastPathComponent.isEmpty || url.lastPathComponent == "/" ? (url.host() ?? url.absoluteString) : url.lastPathComponent
        case .data(_, let fileName): fileName ?? "pasted image"
        }
    }

    /// The formats the server stores; anything else is converted to PNG first.
    static let storedTypes: [UTType] = [.png, .jpeg, .gif, .webP]

    /// What drops and pastes may carry. File URLs before web URLs before raw images.
    static let acceptedTypes: [UTType] = [.fileURL, .url, .image]

    static func isStored(_ type: UTType) -> Bool {
        storedTypes.contains { type.conforms(to: $0) }
    }

    /// Re-encode any image AppKit can read (TIFF, HEIC, BMP...) as PNG.
    static func png(from data: Data) -> Data? {
        guard let rep = NSBitmapImageRep(data: data) else { return nil }
        return rep.representation(using: .png, properties: [:])
    }

    /// The images in a drop or paste, in order. Items that aren't images are skipped.
    @MainActor static func load(_ providers: [NSItemProvider]) async -> [NewImage] {
        var items: [NewImage] = []
        for provider in providers {
            if let item = await load(provider) { items.append(item) }
        }
        return items
    }

    @MainActor static func load(_ provider: NSItemProvider) async -> NewImage? {
        if let url = await url(provider, .fileURL), url.isFileURL {
            return .file(url)
        }
        let image = await imageData(provider)
        if let url = await url(provider, .url), ["http", "https"].contains(url.scheme?.lowercased()) {
            return .web(url, fallback: image)
        }
        return image.map { .data($0, fileName: provider.suggestedName) }
    }

    @MainActor private static func url(_ provider: NSItemProvider, _ type: UTType) async -> URL? {
        guard provider.hasItemConformingToTypeIdentifier(type.identifier),
            let data = try? await data(provider, type)
        else { return nil }
        return URL(dataRepresentation: data, relativeTo: nil)
    }

    /// The provider's image, preferring a format the server stores as is
    /// (AppKit adds TIFF to nearly every image drag).
    @MainActor private static func imageData(_ provider: NSItemProvider) async -> Data? {
        let types = provider.registeredContentTypes.filter { $0.conforms(to: .image) }
        guard let type = types.first(where: isStored) ?? types.first,
            let data = try? await data(provider, type)
        else { return nil }
        return isStored(type) ? data : png(from: data)
    }

    @MainActor private static func data(_ provider: NSItemProvider, _ type: UTType) async throws -> Data {
        try await withCheckedThrowingContinuation { continuation in
            _ = provider.loadDataRepresentation(for: type) { data, error in
                if let data {
                    continuation.resume(returning: data)
                } else {
                    continuation.resume(throwing: error ?? CocoaError(.fileReadUnknown))
                }
            }
        }
    }
}

/// How an add went, as one line for the notice.
struct AddSummary: Equatable {
    var added: [String] = []
    var existing: [String] = []
    var failed: [(label: String, reason: String)] = []

    static func == (a: Self, b: Self) -> Bool {
        a.added == b.added && a.existing == b.existing
            && a.failed.map(\.label) == b.failed.map(\.label) && a.failed.map(\.reason) == b.failed.map(\.reason)
    }

    var message: String {
        var parts: [String] = []
        switch added.count {
        case 0: break
        case 1: parts.append("Added \(added[0])")
        default: parts.append("Added \(added.count) images")
        }
        switch existing.count {
        case 0: break
        case 1 where added.isEmpty && failed.isEmpty: parts.append("\(existing[0]) is already in Kura")
        default: parts.append("\(existing.count) already in Kura")
        }
        switch failed.count {
        case 0: break
        case 1: parts.append("Couldn't add \(failed[0].label): \(failed[0].reason)")
        default: parts.append("Couldn't add \(failed.count): \(failed[0].reason)")
        }
        return parts.joined(separator: " · ")
    }
}
