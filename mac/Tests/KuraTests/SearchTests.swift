import Testing

@testable import Kura

/// The same cases as the TUI's search tests (src/search.rs).
struct SearchTests {
    private func matches(_ query: String, _ name: String, _ tags: [String] = []) -> Bool {
        SearchQuery(query).score(name: name, tags: tags) != nil
    }

    private func image(_ name: String, _ tags: [String] = []) -> KuraImage {
        KuraImage(
            hash: name, name: name, mimeType: "image/png", createdAt: "", tags: tags,
            fileUrl: "", thumbnailUrl: "")
    }

    @Test func emptyQueryMatchesEverything() {
        #expect(SearchQuery("").isEmpty)
        #expect(SearchQuery("  # ").isEmpty)
        #expect(matches("", "anything"))
    }

    @Test func fuzzyMatchesName() {
        #expect(matches("cat", "cat vibing"))
        #expect(matches("cvb", "cat vibing"))
        #expect(!matches("dog", "cat vibing"))
    }

    @Test func fuzzyMatchesTagsIgnoringCaseAndAccents() {
        #expect(matches("reaction", "cat vibing", ["reaction"]))
        #expect(matches("CAFE", "café?"))
    }

    @Test func hashWordsFilterByTagPrefix() {
        #expect(matches("#reac", "cat vibing", ["Reaction"]))
        #expect(!matches("#reac", "cat vibing", ["music"]))
        #expect(matches("#reac #mus", "cat vibing", ["reaction", "music"]))
        #expect(!matches("#reac #mus", "cat vibing", ["reaction"]))
    }

    @Test func tagFilterAndFuzzyCombine() {
        #expect(matches("#reaction cat", "cat vibing", ["reaction"]))
        #expect(!matches("#reaction dog", "cat vibing", ["reaction"]))
    }

    @Test func bestMatchFirstTiesKeepOrder() {
        let images = [image("chocolate"), image("smirk"), image("cat jam"), image("cat nap")]
        #expect(SearchQuery("cat").filter(images).map(\.name) == ["cat jam", "cat nap", "chocolate"])
        #expect(SearchQuery("").filter(images).map(\.name) == images.map(\.name))
    }
}
