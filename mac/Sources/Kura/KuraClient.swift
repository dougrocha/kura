import Foundation

/// An image as `GET /images` returns it (kura-server's `wire::Image`).
struct KuraImage: Decodable, Identifiable, Hashable, Sendable {
    let hash: String
    let name: String
    let mimeType: String?
    let createdAt: String
    let tags: [String]
    let fileUrl: String
    let thumbnailUrl: String

    var id: String { hash }
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

    /// `KURA_URL`, or the server's default address.
    static func fromEnvironment() -> KuraClient {
        let raw = ProcessInfo.processInfo.environment["KURA_URL"] ?? "http://127.0.0.1:7878"
        return KuraClient(baseURL: URL(string: raw) ?? URL(string: "http://127.0.0.1:7878")!)
    }

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

    private func get<T: Decodable>(_ path: String) async throws -> T {
        let (data, response) = try await URLSession.shared.data(from: url(path))
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
            throw (try? decoder.decode(KuraError.self, from: data))
                ?? KuraError(code: "http_\(http.statusCode)", message: "Server answered \(http.statusCode)")
        }
        return try decoder.decode(T.self, from: data)
    }
}
