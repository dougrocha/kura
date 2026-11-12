import CoreGraphics
import Foundation
import ImageIO
import Vision

/// Reads the text written in an image, on this Mac (Apple Vision). Bytes in,
/// string out: it never talks to the server. `CaptionQueue` decides what to
/// read and where the result goes.
struct Captioner: Sendable {
    /// Stored with every ocr caption. Change it when the settings below change in
    /// a way that should re-read the library: `CaptionQueue` re-reads captions
    /// whose engine differs.
    static let engine = "vision:2"  // 2: reads the first, middle and last frames of animations

    static let maxPixels = 2560
    static let minConfidence: Float = 0.3

    enum Failure: Error, LocalizedError {
        case undecodable
        case recognition(String)

        var errorDescription: String? {
            switch self {
            case .undecodable: "The image can't be decoded"
            case .recognition(let message): "Text recognition failed: \(message)"
            }
        }
    }

    /// One recognized line and where it is, in Vision's coordinates (origin at the
    /// bottom left, 0 to 1).
    struct Line: Equatable, Sendable {
        let text: String
        let box: CGRect
    }

    /// The text in reading order, lines joined by "\n". An image with no text
    /// gives "" (an answer, not a failure). The server normalizes whitespace and
    /// length, so this doesn't.
    ///
    /// Animations often show their text partway through, so a GIF or animated
    /// WebP is read at its first, middle and last frames. Each frame's lines
    /// are kept in reading order, and a line already seen (ignoring case) isn't
    /// repeated, so text that stays on screen appears once.
    func read(_ data: Data) async throws -> String {
        let frames = Self.frames(data)
        guard !frames.isEmpty else { throw Failure.undecodable }
        var seen = Set<String>()
        var text: [String] = []
        for frame in frames {
            for line in try recognize(frame) where seen.insert(line.lowercased()).inserted {
                text.append(line)
            }
        }
        return text.joined(separator: "\n")
    }

    /// One frame's lines, in reading order.
    private func recognize(_ image: CGImage) throws -> [String] {
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        // Memes are slang and capital letters; correction mangles them.
        request.usesLanguageCorrection = false
        request.automaticallyDetectsLanguage = true
        do {
            try VNImageRequestHandler(cgImage: image).perform([request])
        } catch {
            throw Failure.recognition(error.localizedDescription)
        }
        let lines = (request.results ?? []).compactMap { observation -> Line? in
            guard let top = observation.topCandidates(1).first, top.confidence >= Self.minConfidence else { return nil }
            return Line(text: top.string, box: observation.boundingBox)
        }
        return Self.ordered(lines)
    }

    /// The frames worth reading: the first, middle and last (just the one for a
    /// still image), each decoded like `decode`.
    static func frames(_ data: Data) -> [CGImage] {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil) else { return [] }
        let count = CGImageSourceGetCount(source)
        guard count > 0 else { return [] }
        var indices: [Int] = []
        for index in [0, count / 2, count - 1] where !indices.contains(index) { indices.append(index) }
        return indices.compactMap { decode(source, at: $0) }
    }

    /// Frame 0, upright, at most `maxPixels` on the long side (never enlarged).
    static func decode(_ data: Data) -> CGImage? {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil), CGImageSourceGetCount(source) > 0 else {
            return nil
        }
        return decode(source, at: 0)
    }

    private static func decode(_ source: CGImageSource, at index: Int) -> CGImage? {
        let properties = CGImageSourceCopyPropertiesAtIndex(source, index, nil) as? [CFString: Any]
        let width = properties?[kCGImagePropertyPixelWidth] as? Int ?? 0
        let height = properties?[kCGImagePropertyPixelHeight] as? Int ?? 0
        guard max(width, height) > maxPixels else {
            return CGImageSourceCreateImageAtIndex(source, index, nil)
        }
        return CGImageSourceCreateThumbnailAtIndex(
            source, index,
            [
                kCGImageSourceCreateThumbnailFromImageAlways: true,
                kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceThumbnailMaxPixelSize: maxPixels,
            ] as CFDictionary)
    }

    /// Top to bottom, then left to right. Lines whose centers are within half a
    /// line height of the row's first line share a row, so two captions side by
    /// side read left to right.
    static func ordered(_ lines: [Line]) -> [String] {
        var rows: [[Line]] = []
        for line in lines.sorted(by: { $0.box.midY > $1.box.midY }) {
            if let first = rows.last?.first,
                abs(first.box.midY - line.box.midY) < 0.5 * min(first.box.height, line.box.height)
            {
                rows[rows.count - 1].append(line)
            } else {
                rows.append([line])
            }
        }
        return rows.flatMap { $0.sorted { $0.box.minX < $1.box.minX } }.map(\.text)
    }
}
