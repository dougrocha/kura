import AVFoundation
import CoreVideo
import Foundation
import ImageIO
import Testing

@testable import Kura

struct VideoGIFTests {
    /// Writes a real H.264 MP4: `seconds` long at 30 fps, `width`x`height`,
    /// each frame a different gray so frames aren't identical.
    static func mp4(seconds: Double, width: Int = 640, height: Int = 480) async throws -> URL {
        let url = FileManager.default.temporaryDirectory.appending(path: "kura-video-\(UUID().uuidString).mp4")
        let writer = try AVAssetWriter(outputURL: url, fileType: .mp4)
        let input = AVAssetWriterInput(
            mediaType: .video,
            outputSettings: [AVVideoCodecKey: AVVideoCodecType.h264, AVVideoWidthKey: width, AVVideoHeightKey: height])
        let adaptor = AVAssetWriterInputPixelBufferAdaptor(
            assetWriterInput: input,
            sourcePixelBufferAttributes: [
                kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
                kCVPixelBufferWidthKey as String: width, kCVPixelBufferHeightKey as String: height,
            ])
        writer.add(input)
        writer.startWriting()
        writer.startSession(atSourceTime: .zero)
        let frames = Int(seconds * 30)
        for index in 0..<frames {
            while !input.isReadyForMoreMediaData { try await Task.sleep(for: .milliseconds(5)) }
            var buffer: CVPixelBuffer?
            CVPixelBufferPoolCreatePixelBuffer(nil, adaptor.pixelBufferPool!, &buffer)
            let pixels = buffer!
            CVPixelBufferLockBaseAddress(pixels, [])
            memset(
                CVPixelBufferGetBaseAddress(pixels), Int32(index * 255 / max(frames - 1, 1)),
                CVPixelBufferGetDataSize(pixels))
            CVPixelBufferUnlockBaseAddress(pixels, [])
            adaptor.append(pixels, withPresentationTime: CMTime(value: CMTimeValue(index), timescale: 30))
        }
        input.markAsFinished()
        await writer.finishWriting()
        #expect(writer.status == .completed)
        return url
    }

    private func frames(_ gif: Data) -> (count: Int, width: Int, height: Int) {
        let source = CGImageSourceCreateWithData(gif as CFData, nil)!
        let first = CGImageSourceCreateImageAtIndex(source, 0, nil)!
        return (CGImageSourceGetCount(source), first.width, first.height)
    }

    @Test func aClipBecomesALoopingGIFAt12FPSAndAtMost480Wide() async throws {
        let video = try await Self.mp4(seconds: 1.5)
        defer { try? FileManager.default.removeItem(at: video) }
        let gif = try await VideoGIF.make(from: video)
        #expect(gif.starts(with: Array("GIF8".utf8)))
        let (count, width, height) = frames(gif)
        #expect(count == 18)  // 1.5 s at 12 fps
        #expect(width == 480)
        #expect(height == 360)  // 4:3 kept
    }

    @Test func longClipsAreCutAt15Seconds() async throws {
        let video = try await Self.mp4(seconds: 17, width: 64, height: 48)
        defer { try? FileManager.default.removeItem(at: video) }
        #expect(frames(try await VideoGIF.make(from: video)).count == 180)
    }

    @Test func videosAreRecognizedByExtension() {
        #expect(VideoGIF.isVideo(URL(fileURLWithPath: "/x/clip.mp4")))
        #expect(VideoGIF.isVideo(URL(fileURLWithPath: "/x/clip.MOV")))
        #expect(!VideoGIF.isVideo(URL(fileURLWithPath: "/x/cat.gif")))
    }

    @Test func aFileThatIsNotReallyAVideoFails() async throws {
        let fake = FileManager.default.temporaryDirectory.appending(path: "kura-fake-\(UUID().uuidString).mp4")
        try Data("not a video".utf8).write(to: fake)
        defer { try? FileManager.default.removeItem(at: fake) }
        await #expect(throws: (any Error).self) { try await VideoGIF.make(from: fake) }
    }

    /// Against a real server: set KURA_BIN to a built kura-server (`just mac-test` does).
    @Test(.enabled(if: ProcessInfo.processInfo.environment["KURA_BIN"] != nil)) @MainActor
    func addingAnMP4StoresAGIFNamedAfterTheClip() async throws {
        let folder = FileManager.default.temporaryDirectory.appending(path: "kura-video-lib-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let server = ServerProcess()
        defer {
            server.stop()
            try? FileManager.default.removeItem(at: folder)
        }
        let client = KuraClient(baseURL: try await server.start(library: folder.appending(path: "lib").path))
        let video = try await Self.mp4(seconds: 1, width: 160, height: 120)
        let clip = folder.appending(path: "dancing cat.mp4")
        try FileManager.default.moveItem(at: video, to: clip)

        let added = try await client.add(.file(clip))
        #expect(added.created)
        #expect(added.image.mimeType == "image/gif")
        #expect(added.image.name == "dancing-cat")
    }
}
