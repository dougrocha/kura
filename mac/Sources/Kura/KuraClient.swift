import Foundation
import UniformTypeIdentifiers

/// An image as `GET /images` returns it (kura-server's `wire::Image`).
struct KuraImage: Decodable, Identifiable, Hashable, Sendable {
    let hash: String
    let name: String
    let mimeType: String?
    let createdAt: String
    let tags: [String]
    let fileUrl: String
    let thumbnailUrl: String
    /// The text read from (or typed for) the image; nil until it has been read.
    /// A `var` with a default keeps the memberwise init and `Decodable` working,
    /// and a server that doesn't send the key decodes as nil.
    var caption: KuraCaption? = nil

    var id: String { hash }
}

/// The text in an image (kura-server's `wire::Caption`).
struct KuraCaption: Decodable, Hashable, Sendable {
    /// Empty means the image was read and has no text.
    let text: String
    /// `ocr` (read on this Mac) or `manual` (typed by a person, wins over ocr).
    let source: String
    /// The recognizer that wrote an ocr caption, e.g. `vision:1`.
    let engine: String?

    var isManual: Bool { source == "manual" }
}

enum CaptionSource: String, Sendable {
    case ocr, manual
}

/// `GET /search`: ranked hashes, best first. The Mac keeps the images themselves.
struct SearchResponse: Decodable, Equatable, Sendable {
    let seq: UInt64
    let hits: [SearchHit]
    /// How many images the library has.
    let images: Int
    /// How many of them have no caption yet.
    let unread: Int
}

struct SearchHit: Decodable, Equatable, Sendable {
    let hash: String
    /// Present when the image's text, not its name or tags, matched.
    let snippet: Snippet?
}

struct Snippet: Decodable, Equatable, Sendable {
    struct Segment: Decodable, Equatable, Sendable {
        let text: String
        let matched: Bool
    }

    let leadingCut: Bool
    let segments: [Segment]
    let trailingCut: Bool

    /// The snippet as one line, matches bold, with an ellipsis at a cut end.
    var attributed: AttributedString {
        var result = AttributedString(leadingCut ? "…" : "")
        for segment in segments {
            var part = AttributedString(segment.text)
            if segment.matched { part.inlinePresentationIntent = .stronglyEmphasized }
            result += part
        }
        if trailingCut { result += AttributedString("…") }
        return result
    }
}

/// What the UI needs from a failed edit, parsed once from the error's code.
/// Views switch on this, never on strings.
enum EditFailure: Equatable, Sendable {
    /// 409 `name_taken`
    case nameTaken
    /// 400 `invalid_input`, with the server's message
    case invalid(String)
    /// 404 `not_found`: it was deleted somewhere else
    case gone
    /// Anything else: the server isn't reachable, or answered with something unexpected
    case unavailable(String)

    init(_ error: Error) {
        if let error = error as? KuraError {
            switch error.code {
            case "name_taken": self = .nameTaken
            case "invalid_input", "invalid_request": self = .invalid(error.message)
            case "not_found": self = .gone
            default: self = .unavailable(error.message)
            }
        } else {
            self = .unavailable(error.localizedDescription)
        }
    }

    /// Words for a failure shown in the UI.
    func message(for name: String? = nil) -> String {
        switch self {
        case .nameTaken: name.map { "Another image is already named “\($0)”." } ?? "That name is already taken."
        case .invalid(let message), .unavailable(let message): message
        case .gone: "This image was deleted."
        }
    }
}

/// The shared config file (`kura config`), as `GET /config` returns it.
struct KuraConfig: Decodable, Equatable, Sendable {
    let path: String
    /// As written in the file (`~/Memes`); nil means the default library.
    let library: String?
    /// The images folder the running server uses.
    let libraryFolder: String
    var closeAfterCopy: Bool
    var relaunchWhenHidden: Bool
}

/// Every error response (kura-server's `wire::ErrorBody`).
struct KuraError: Decodable, LocalizedError, Sendable {
    let code: String
    let message: String

    var errorDescription: String? { message }
}

/// A thin client for the kura-server HTTP API.
struct KuraClient: Sendable {
    let baseURL: URL

    func images() async throws -> [KuraImage] {
        try await get("/images")
    }

    /// The server returns paths like `/images/<hash>/thumbnail`; make them absolute.
    func url(_ path: String, query: [URLQueryItem] = []) -> URL {
        var url = baseURL.appending(path: path)
        if !query.isEmpty { url.append(queryItems: query) }
        return url
    }

    func thumbnailURL(_ image: KuraImage, size: String) -> URL {
        url(image.thumbnailUrl, query: [URLQueryItem(name: "size", value: size)])
    }

    func config() async throws -> KuraConfig {
        try await get("/config")
    }

    /// Like `kura config set`; nil is `kura config unset`.
    func setConfig(_ key: String, to value: String?) async throws -> KuraConfig {
        var request = URLRequest(url: url("/config"))
        request.httpMethod = "PATCH"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONSerialization.data(withJSONObject: ["key": key, "value": value as Any? ?? NSNull()] as [String: Any])
        return try Self.decoder.decode(KuraConfig.self, from: try await send(request))
    }

    // MARK: Search

    func search(_ query: String) async throws -> SearchResponse {
        try Self.decoder.decode(SearchResponse.self, from: try await send(URLRequest(url: searchURL(query))))
    }

    /// `?q=` with every reserved character escaped: the query language uses `#`, `'`,
    /// `!`, `^` and `$`, and a bare `+` or `&` would be read as a space or a new parameter.
    func searchURL(_ query: String) -> URL {
        var components = URLComponents(url: baseURL, resolvingAgainstBaseURL: false)!
        components.percentEncodedPath = Self.basePath(of: baseURL) + "/search"
        components.percentEncodedQuery = "q=" + Self.encode(query)
        return components.url!
    }

    // MARK: Edits (always by hash: it never needs escaping, and a rename elsewhere can't redirect an edit)

    func rename(_ hash: String, to name: String) async throws -> KuraImage {
        try await edit("PATCH", ["images", hash], ["name": name])
    }

    func addTag(_ hash: String, _ tag: String) async throws -> KuraImage {
        try await edit("POST", ["images", hash, "tags"], ["tag": tag])
    }

    func removeTag(_ hash: String, _ tag: String) async throws -> KuraImage {
        try await edit("DELETE", ["images", hash, "tags", tag])
    }

    /// 204 with no body. A 404 throws `KuraError(code: "not_found")` like any other.
    func delete(_ hash: String) async throws {
        _ = try await send(request("DELETE", ["images", hash]))
    }

    /// Store text for an image. `engine` is required for `.ocr` and refused for `.manual`.
    func setCaption(_ hash: String, source: CaptionSource, text: String, engine: String? = nil) async throws -> KuraImage {
        var body = ["text": text]
        if let engine { body["engine"] = engine }
        return try await edit("PUT", ["images", hash, "captions", source.rawValue], body)
    }

    func clearCaption(_ hash: String, source: CaptionSource) async throws -> KuraImage {
        try await edit("DELETE", ["images", hash, "captions", source.rawValue])
    }

    private func edit(_ method: String, _ components: [String], _ body: [String: String]? = nil) async throws -> KuraImage {
        var request = request(method, components)
        if let body {
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = try JSONSerialization.data(withJSONObject: body)
        }
        return try Self.decoder.decode(KuraImage.self, from: try await send(request))
    }

    private func request(_ method: String, _ components: [String]) -> URLRequest {
        var request = URLRequest(url: editURL(components))
        request.httpMethod = method
        return request
    }

    /// `/images/<hash>/tags/<tag>`: each component is raw and is encoded exactly once.
    /// (`URL.appending(path:)` would treat a `/` in a tag as a separator.)
    func editURL(_ components: [String]) -> URL {
        var url = URLComponents(url: baseURL, resolvingAgainstBaseURL: false)!
        url.percentEncodedPath = Self.basePath(of: baseURL) + components.map { "/" + Self.encode($0) }.joined()
        return url.url!
    }

    private static func basePath(of url: URL) -> String {
        var path = URLComponents(url: url, resolvingAgainstBaseURL: false)?.percentEncodedPath ?? ""
        while path.hasSuffix("/") { path.removeLast() }
        return path
    }

    private static let unreserved = CharacterSet(
        charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~")

    /// One path segment or query value. Only RFC 3986 unreserved characters stay
    /// literal, so `/ ? # & + %`, spaces and non-ASCII are all escaped.
    static func encode(_ raw: String) -> String {
        raw.addingPercentEncoding(withAllowedCharacters: unreserved) ?? raw
    }

    /// The original file's bytes.
    func file(_ image: KuraImage) async throws -> Data {
        try await data(image.fileUrl)
    }

    /// An image the library has after an add; `created` is false when it
    /// already had those bytes (the server answers 200 instead of 201).
    struct Added: Sendable {
        let image: KuraImage
        let created: Bool
    }

    /// Add one image: `POST /images` for files and bytes, `POST /imports` for
    /// web URLs. Formats the server doesn't store are converted to PNG here.
    func add(_ item: NewImage) async throws -> Added {
        switch item {
        case .file(let file):
            let type = UTType(filenameExtension: file.pathExtension)
            if let type, NewImage.isStored(type) {
                var request = uploadRequest(fileName: file.lastPathComponent)
                request.httpMethod = "POST"
                return try decodeAdded(try await URLSession.shared.upload(for: request, fromFile: file))
            }
            if VideoGIF.isVideo(file) {
                let gif = try await VideoGIF.make(from: file)
                let name = file.deletingPathExtension().lastPathComponent + ".gif"
                return try await upload(gif, fileName: name)
            }
            // HEIC, TIFF, or no extension: read it, and convert if it isn't stored as is.
            let data = try Data(contentsOf: file)
            return try await upload(data, fileName: file.lastPathComponent)
        case .web(let web, let fallback):
            do {
                var request = URLRequest(url: url("/imports"))
                request.httpMethod = "POST"
                request.setValue("application/json", forHTTPHeaderField: "Content-Type")
                request.httpBody = try JSONSerialization.data(withJSONObject: ["url": web.absoluteString])
                return try decodeAdded(try await URLSession.shared.data(for: request))
            } catch {
                guard let fallback else { throw error }
                return try await upload(fallback, fileName: web.lastPathComponent)
            }
        case .data(let data, let fileName):
            return try await upload(data, fileName: fileName)
        }
    }

    private func upload(_ data: Data, fileName: String?) async throws -> Added {
        var body = data
        var fileName = fileName
        if !Self.isStored(data) {
            guard let png = NewImage.png(from: data) else {
                throw KuraError(code: "unsupported_format", message: "Kura stores PNG, JPEG, GIF and WebP images")
            }
            body = png
            fileName = fileName.map { (($0 as NSString).deletingPathExtension as String) + ".png" }
        }
        var request = uploadRequest(fileName: fileName)
        request.httpMethod = "POST"
        return try decodeAdded(try await URLSession.shared.upload(for: request, from: body))
    }

    /// The server sniffs the first bytes; so do we, to know when to convert.
    private static func isStored(_ data: Data) -> Bool {
        let head = [UInt8](data.prefix(12))
        func starts(_ magic: [UInt8]) -> Bool { head.starts(with: magic) }
        return starts([0x89, 0x50, 0x4E, 0x47]) || starts([0xFF, 0xD8, 0xFF]) || starts(Array("GIF8".utf8))
            || (starts(Array("RIFF".utf8)) && head.count == 12 && Array(head[8..<12]) == Array("WEBP".utf8))
    }

    private func uploadRequest(fileName: String?) -> URLRequest {
        let query = fileName.map { [URLQueryItem(name: "file_name", value: $0)] } ?? []
        return URLRequest(url: url("/images", query: query))
    }

    private func decodeAdded(_ result: (Data, URLResponse)) throws -> Added {
        let (data, response) = result
        try Self.check(data, response)
        let status = (response as? HTTPURLResponse)?.statusCode
        return Added(image: try Self.decoder.decode(KuraImage.self, from: data), created: status == 201)
    }

    private func get<T: Decodable>(_ path: String) async throws -> T {
        let data = try await data(path)
        return try Self.decoder.decode(T.self, from: data)
    }

    private func data(_ path: String) async throws -> Data {
        try await send(URLRequest(url: url(path)))
    }

    private func send(_ request: URLRequest) async throws -> Data {
        let (data, response) = try await URLSession.shared.data(for: request)
        try Self.check(data, response)
        return data
    }

    private static func check(_ data: Data, _ response: URLResponse) throws {
        if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
            throw (try? decoder.decode(KuraError.self, from: data))
                ?? KuraError(code: "http_\(http.statusCode)", message: "Server answered \(http.statusCode)")
        }
    }

    private static var decoder: JSONDecoder {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return decoder
    }
}
