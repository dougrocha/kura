import Testing

@testable import Kura

/// The TUI's search tests (src/search.rs), then nucleo behavior they rely on.
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

    @Test func fuzzyMatchesTags() {
        #expect(matches("reaction", "cat vibing", ["reaction"]))
    }

    // Below: nucleo behavior the TUI has that src/search.rs doesn't test.

    @Test func smartCase() {
        #expect(matches("cat", "Cat Jam"))
        #expect(!matches("Cat", "cat jam"))
        #expect(matches("Cat", "Cat jam"))
    }

    @Test func smartAccents() {
        #expect(matches("cafe", "café?"))
        #expect(!matches("café", "cafe"))
        #expect(matches("café", "café?"))
        #expect(matches("#cafe", "x", ["Cafe"]))
        #expect(!matches("#café", "x", ["cafe"]))
    }

    @Test func fzfOperators() {
        #expect(!matches("!cat", "cat jam"))
        #expect(matches("!cat", "dog"))
        #expect(!matches("!cat", "the cats"))  // negation is a substring match
        #expect(matches("'at", "cat jam"))
        #expect(!matches("'cj", "cat jam"))
        #expect(matches("^cat", "cat jam"))
        #expect(!matches("^jam", "cat jam"))
        #expect(matches("jam$", "cat jam"))
        #expect(!matches("cat$", "cat jam"))
        #expect(matches("^cat$", "cat"))
        #expect(!matches("^cat$", "cat jam"))
        #expect(matches("\\!cat", "!cat"))
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
