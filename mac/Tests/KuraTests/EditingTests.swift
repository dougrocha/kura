import Foundation
import Testing

@testable import Kura

struct EditingTests {
    private let client = KuraClient(baseURL: URL(string: "http://127.0.0.1:7878")!)
    private let hash = String(repeating: "ab", count: 32)

    // MARK: path encoding

    @Test func aSlashInATagStaysInOneSegment() {
        #expect(KuraClient.encode("a/b") == "a%2Fb")
        #expect(client.editURL(["images", hash, "tags", "a/b"]).absoluteString
            == "http://127.0.0.1:7878/images/\(hash)/tags/a%2Fb")
    }

    @Test func onlyUnreservedCharactersStayLiteral() {
        #expect(KuraClient.encode("AZaz09-._~") == "AZaz09-._~")
        #expect(KuraClient.encode("a b?c#d%e&f+g") == "a%20b%3Fc%23d%25e%26f%2Bg")
        #expect(KuraClient.encode("café 猫") == "caf%C3%A9%20%E7%8C%AB")
    }

    @Test func aSegmentIsEncodedExactlyOnce() {
        let url = client.editURL(["images", hash, "tags", "100%"])
        #expect(url.absoluteString.hasSuffix("/tags/100%25"))
        #expect(url.path(percentEncoded: false).hasSuffix("/tags/100%"))
    }

    @Test func aBasePathIsKept() {
        let proxied = KuraClient(baseURL: URL(string: "http://example.com/kura/")!)
        #expect(proxied.editURL(["images", "x"]).absoluteString == "http://example.com/kura/images/x")
        #expect(proxied.searchURL("cat").absoluteString == "http://example.com/kura/search?q=cat")
    }

    @Test func theSearchQueryEscapesWhatTheQueryLanguageUses() {
        let url = client.searchURL("#cat 'moon !dog ^a$ a+b&c é")
        #expect(url.absoluteString
            == "http://127.0.0.1:7878/search?q=%23cat%20%27moon%20%21dog%20%5Ea%24%20a%2Bb%26c%20%C3%A9")
        let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems
        #expect(items == [URLQueryItem(name: "q", value: "#cat 'moon !dog ^a$ a+b&c é")])
    }

    // MARK: failures

    private func failure(_ code: String, _ message: String = "m") -> EditFailure {
        EditFailure(KuraError(code: code, message: message))
    }

    @Test func failuresAreParsedFromTheErrorCode() {
        #expect(failure("name_taken") == .nameTaken)
        #expect(failure("invalid_input", "name too long") == .invalid("name too long"))
        #expect(failure("invalid_request", "bad json") == .invalid("bad json"))
        #expect(failure("not_found") == .gone)
        #expect(failure("internal", "boom") == .unavailable("boom"))
        #expect(failure("http_502", "Server answered 502") == .unavailable("Server answered 502"))
        #expect(EditFailure(URLError(.cannotConnectToHost)) == .unavailable(URLError(.cannotConnectToHost).localizedDescription))
    }

    @Test func failureMessages() {
        #expect(EditFailure.nameTaken.message(for: "cat") == "Another image is already named “cat”.")
        #expect(EditFailure.invalid("too long").message() == "too long")
    }

    // MARK: wire JSON

    private func decode<T: Decodable>(_ type: T.Type, _ json: String) throws -> T {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode(type, from: Data(json.utf8))
    }

    private func imageJSON(caption: String) -> String {
        """
        {"hash":"\(hash)","name":"cat","mime_type":"image/gif","created_at":"2026-01-01T00:00:00Z",
         "tags":["a"],"file_url":"/images/\(hash)/file","thumbnail_url":"/images/\(hash)/thumbnail",
         "caption":\(caption)}
        """
    }

    @Test func anUnreadImageHasNoCaption() throws {
        #expect(try decode(KuraImage.self, imageJSON(caption: "null")).caption == nil)
        let missing = """
            {"hash":"h","name":"n","mime_type":null,"created_at":"","tags":[],"file_url":"","thumbnail_url":""}
            """
        #expect(try decode(KuraImage.self, missing).caption == nil)
    }

    @Test func captionsDecodeWithTheirSource() throws {
        let ocr = try decode(
            KuraImage.self, imageJSON(caption: #"{"text":"when the moon","source":"ocr","engine":"vision:1"}"#))
        #expect(ocr.caption == KuraCaption(text: "when the moon", source: "ocr", engine: "vision:1"))
        let manual = try decode(KuraImage.self, imageJSON(caption: #"{"text":"","source":"manual","engine":null}"#))
        #expect(manual.caption?.isManual == true)
        #expect(manual.caption?.text == "")
        let noEngine = try decode(KuraImage.self, imageJSON(caption: #"{"text":"x","source":"manual"}"#))
        #expect(noEngine.caption?.engine == nil)
    }

    @Test func searchResponsesDecode() throws {
        let json = """
            {"seq":42,"images":3,"unread":1,"hits":[
              {"hash":"a"},
              {"hash":"b","snippet":{"leading_cut":true,"trailing_cut":false,"segments":[
                {"text":"when the ","matched":false},{"text":"moon","matched":true}]}}]}
            """
        let response = try decode(SearchResponse.self, json)
        #expect(response.seq == 42)
        #expect(response.images == 3)
        #expect(response.unread == 1)
        #expect(response.hits.map(\.hash) == ["a", "b"])
        #expect(response.hits[0].snippet == nil)
        let snippet = try #require(response.hits[1].snippet)
        #expect(snippet.leadingCut && !snippet.trailingCut)
        #expect(snippet.segments.last == Snippet.Segment(text: "moon", matched: true))
    }

    @Test func aSnippetIsOneLineWithTheMatchBold() {
        let snippet = Snippet(
            leadingCut: true,
            segments: [.init(text: "when the ", matched: false), .init(text: "moon", matched: true)],
            trailingCut: true)
        let line = snippet.attributed
        #expect(String(line.characters) == "…when the moon…")
        let bold = line.runs.filter { $0.inlinePresentationIntent == .stronglyEmphasized }
        #expect(bold.map { String(line[$0.range].characters) } == ["moon"])
    }
}
