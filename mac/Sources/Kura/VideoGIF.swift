import AVFoundation
import Foundation
import ImageIO
import UniformTypeIdentifiers

/// Turns a video clip (MP4, MOV, ...) into an animated GIF, so it can live in
/// the library like any other meme: thumbnails, previews, text reading and
/// copy all work on GIFs. Kura doesn't store video itself.
///
/// GIFs are big (256 colors, no inter-frame compression), so the clip is
/// sampled: at most `maxSeconds` from the start, `fps` frames a second, and
/// no wider or taller than `maxSide`. That keeps a typical clip at a few MB.
enum VideoGIF {
    static let maxSeconds = 15.0
    static let fps = 12.0
    static let maxSide: CGFloat = 480

    /// Whether a file is a video to convert, judging by its extension.
    static func isVideo(_ file: URL) -> Bool {
        UTType(filenameExtension: file.pathExtension)?.conforms(to: .movie) ?? false
    }

    static func make(from file: URL) async throws -> Data {
        let asset = AVURLAsset(url: file)
        guard try await !asset.loadTracks(withMediaType: .video).isEmpty else {
            throw KuraError(code: "unsupported_format", message: "That video has no picture to turn into a GIF")
        }
        let seconds = min(try await asset.load(.duration).seconds, maxSeconds)
        guard seconds.isFinite, seconds > 0 else {
            throw KuraError(code: "unsupported_format", message: "That video is empty")
        }

        let generator = AVAssetImageGenerator(asset: asset)
        generator.appliesPreferredTrackTransform = true  // phone videos stay upright
        generator.maximumSize = CGSize(width: maxSide, height: maxSide)  // keeps the aspect ratio
        // Each frame may come from anywhere within its slot: much faster than
        // decoding to exact times, and invisible at 12 fps.
        let slot = CMTime(seconds: 0.5 / fps, preferredTimescale: 600)
        generator.requestedTimeToleranceBefore = slot
        generator.requestedTimeToleranceAfter = slot

        let count = max(1, Int((seconds * fps).rounded(.down)))
        let times = (0..<count).map { CMTime(seconds: Double($0) / fps, preferredTimescale: 600) }
        var frames: [CGImage] = []
        for await result in generator.images(for: times) {
            // A frame that can't be decoded (a damaged spot) is skipped, not fatal.
            if let image = try? result.image { frames.append(image) }
        }
        guard !frames.isEmpty else {
            throw KuraError(code: "unsupported_format", message: "Couldn't read any frames from that video")
        }
        return try gif(frames, delay: 1 / fps)
    }

    /// An endlessly looping GIF of `frames`, each shown for `delay` seconds.
    static func gif(_ frames: [CGImage], delay: Double) throws -> Data {
        let data = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(data, UTType.gif.identifier as CFString, frames.count, nil)
        else { throw KuraError(code: "internal", message: "Couldn't create a GIF") }
        let loop = [kCGImagePropertyGIFDictionary: [kCGImagePropertyGIFLoopCount: 0]]
        CGImageDestinationSetProperties(destination, loop as CFDictionary)
        let frame = [kCGImagePropertyGIFDictionary: [
            kCGImagePropertyGIFDelayTime: delay, kCGImagePropertyGIFUnclampedDelayTime: delay,
        ]]
        for image in frames {
            CGImageDestinationAddImage(destination, image, frame as CFDictionary)
        }
        guard CGImageDestinationFinalize(destination) else {
            throw KuraError(code: "internal", message: "Couldn't write the GIF")
        }
        return data as Data
    }
}
