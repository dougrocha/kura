import Foundation

/// The TUI's search (`src/search.rs`), for the sidebar's search field.
///
/// Words starting with `#` keep only images that have a tag starting with that
/// word. Every other word must fuzzy match the image's name or tags: its
/// letters in order, not necessarily together. Matching ignores case and
/// accents. Best matches come first; ties keep the library's order.
struct SearchQuery: Equatable {
    private(set) var tagPrefixes: [String] = []
    private(set) var words: [String] = []

    init(_ text: String) {
        for word in text.split(whereSeparator: \.isWhitespace).map(String.init) {
            if word.hasPrefix("#") {
                let tag = Self.fold(String(word.dropFirst()))
                if !tag.isEmpty { tagPrefixes.append(tag) }
            } else {
                words.append(Self.fold(word))
            }
        }
    }

    var isEmpty: Bool { tagPrefixes.isEmpty && words.isEmpty }

    /// The images that match, best first.
    func filter(_ images: [KuraImage]) -> [KuraImage] {
        if isEmpty { return images }
        return images.enumerated()
            .compactMap { offset, image in score(name: image.name, tags: image.tags).map { (offset, image, $0) } }
            .sorted { $0.2 != $1.2 ? $0.2 > $1.2 : $0.0 < $1.0 }
            .map(\.1)
    }

    /// nil when the image doesn't match; higher is a better match.
    func score(name: String, tags: [String]) -> Int? {
        let foldedTags = tags.map(Self.fold)
        let hasTags = tagPrefixes.allSatisfy { wanted in foldedTags.contains { $0.hasPrefix(wanted) } }
        guard hasTags else { return nil }

        let haystack = Array(([Self.fold(name)] + foldedTags).joined(separator: " "))
        var total = 0
        for word in words {
            guard let score = Self.fuzzyScore(Array(word), in: haystack) else { return nil }
            total += score
        }
        return total
    }

    /// Letters of `needle` in order within `haystack`. Consecutive letters and
    /// letters at the start of a word score higher, so "cat" ranks "cat jam"
    /// above "chocolate".
    private static func fuzzyScore(_ needle: [Character], in haystack: [Character]) -> Int? {
        var score = 0
        var index = 0
        var previous: Int?
        for letter in needle {
            while index < haystack.count, haystack[index] != letter { index += 1 }
            guard index < haystack.count else { return nil }
            score += 1
            if let previous, previous == index - 1 { score += 4 }
            if index == 0 || haystack[index - 1] == " " { score += 3 }
            previous = index
            index += 1
        }
        return score
    }

    private static func fold(_ text: String) -> String {
        text.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: nil)
    }
}
