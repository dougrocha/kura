import Foundation

/// Which memes you actually use, so Quick Pick can show them first. Kept on
/// this Mac only (UserDefaults): it's a convenience, not library data.
@MainActor
final class Usage {
    struct Entry: Codable, Equatable, Sendable {
        var count: Int
        var last: Date
    }

    private let defaults: UserDefaults
    private let key = "quickPick.usage"
    private(set) var entries: [String: Entry]

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        entries = (defaults.data(forKey: key)).flatMap { try? JSONDecoder().decode([String: Entry].self, from: $0) } ?? [:]
    }

    func record(_ hash: String, at now: Date = .now) {
        var entry = entries[hash] ?? Entry(count: 0, last: now)
        entry.count += 1
        entry.last = now
        entries[hash] = entry
        if let data = try? JSONEncoder().encode(entries) { defaults.set(data, forKey: key) }
    }

    /// Frecency: every use counts, recent ones count more (a use loses half
    /// its weight each week). Unused images score 0.
    nonisolated static func score(_ entry: Entry?, now: Date) -> Double {
        guard let entry else { return 0 }
        let weeks = max(now.timeIntervalSince(entry.last), 0) / (7 * 24 * 3600)
        return Double(entry.count) * pow(0.5, weeks)
    }

    /// Used images first, most frecent first; then the rest in library order
    /// (newest first). Stable, so ties keep library order.
    nonisolated static func ordered(_ images: [KuraImage], by entries: [String: Entry], now: Date = .now) -> [KuraImage] {
        images.enumerated()
            .map { (offset: $0.offset, image: $0.element, score: score(entries[$0.element.hash], now: now)) }
            .sorted { $0.score != $1.score ? $0.score > $1.score : $0.offset < $1.offset }
            .map(\.image)
    }
}
