import Foundation

/// The TUI's search (`src/search.rs`, nucleo with fzf syntax), for the sidebar.
///
/// `#word` keeps only images with a tag starting with `word` (ignoring case).
/// Every other word is matched against the image's name and tags:
///
/// - `cat` fuzzy: the letters in order, not necessarily together
/// - `'cat` substring, `^cat` prefix, `cat$` suffix, `^cat$` exact
/// - `!cat` excludes images containing "cat" (`!^cat`, `!cat$` also work)
/// - a `\` before `!`, `^`, `'` or a final `$` makes it a plain character
///
/// Smart case and accents, as in nucleo: a word with a capital letter matches
/// case exactly, and a word with an accent matches accents exactly.
/// Best matches come first; ties keep the library's order.
struct SearchQuery: Equatable {
    private(set) var tagPrefixes: [String] = []
    private(set) var atoms: [Atom] = []

    struct Atom: Equatable {
        enum Kind { case fuzzy, substring, prefix, suffix, exact }
        var needle: String
        var kind: Kind
        var negative: Bool
    }

    init(_ text: String) {
        for word in text.split(whereSeparator: \.isWhitespace).map(String.init) {
            if word.hasPrefix("#") {
                let tag = String(word.dropFirst()).lowercased()
                if !tag.isEmpty { tagPrefixes.append(tag) }
            } else if let atom = Self.atom(word) {
                atoms.append(atom)
            }
        }
    }

    var isEmpty: Bool { tagPrefixes.isEmpty && atoms.isEmpty }

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
        let lowerTags = tags.map { $0.lowercased() }
        guard tagPrefixes.allSatisfy({ wanted in lowerTags.contains { $0.hasPrefix(wanted) } }) else { return nil }

        let haystack = ([name] + tags).joined(separator: " ")
        var total = 0
        for atom in atoms {
            let score = Self.match(atom, in: haystack)
            if atom.negative {
                if score != nil { return nil }
            } else {
                guard let score else { return nil }
                total += score
            }
        }
        return total
    }

    /// nucleo's `Atom::parse`: `!` first, then `^` or `'`, then a final `$`.
    private static func atom(_ word: String) -> Atom? {
        var rest = Substring(word)
        var negative = false
        if rest.hasPrefix("!") {
            negative = true
            rest = rest.dropFirst()
        } else if rest.hasPrefix("\\!") {
            rest = rest.dropFirst()
        }

        var kind = Atom.Kind.fuzzy
        if rest.hasPrefix("^") {
            kind = .prefix
            rest = rest.dropFirst()
        } else if rest.hasPrefix("'") {
            kind = .substring
            rest = rest.dropFirst()
        } else if rest.hasPrefix("\\^") || rest.hasPrefix("\\'") {
            rest = rest.dropFirst()
        }

        if rest.hasSuffix("\\$") {
            rest = rest.dropLast(2) + "$"
        } else if rest.hasSuffix("$") {
            kind = kind == .fuzzy ? .suffix : .exact
            rest = rest.dropLast()
        }

        if negative && kind == .fuzzy { kind = .substring }
        return rest.isEmpty ? nil : Atom(needle: String(rest), kind: kind, negative: negative)
    }

    private static func match(_ atom: Atom, in haystack: String) -> Int? {
        // Smart case and accents: only fold what the needle doesn't spell out.
        var options: String.CompareOptions = []
        if atom.needle == atom.needle.lowercased() { options.insert(.caseInsensitive) }
        if atom.needle == atom.needle.folding(options: .diacriticInsensitive, locale: nil) {
            options.insert(.diacriticInsensitive)
        }
        let needle = atom.needle.folding(options: options, locale: nil)
        let haystack = haystack.folding(options: options, locale: nil)
        let exactBonus = needle.count * 6

        switch atom.kind {
        case .fuzzy: return fuzzyScore(Array(needle), in: Array(haystack))
        case .substring: return haystack.contains(needle) ? exactBonus : nil
        case .prefix: return haystack.hasPrefix(needle) ? exactBonus : nil
        case .suffix: return haystack.hasSuffix(needle) ? exactBonus : nil
        case .exact: return haystack == needle ? exactBonus * 2 : nil
        }
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
}
