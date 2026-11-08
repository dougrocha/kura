import Foundation

/// The app's own kura server: a child process on a free localhost port.
///
/// The app is standalone, so the server is an implementation detail. It
/// listens on a port the OS picks (no clash with a `kura serve` you run
/// yourself), and is started with `--exit-with-stdin` while the app holds
/// its stdin: if the app quits or crashes, the server exits too.
@MainActor
final class ServerProcess {
    struct Failure: LocalizedError {
        let message: String
        var errorDescription: String? { message }
    }

    private var process: Process?
    private var stdin: Pipe?
    /// Called when the server exits without `stop()` asking it to.
    var onUnexpectedExit: ((String) -> Void)?

    /// The `kura-server` binary: `KURA_BIN`, else the copy inside Kura.app.
    static func binary() -> URL? {
        if let path = ProcessInfo.processInfo.environment["KURA_BIN"], !path.isEmpty {
            return URL(fileURLWithPath: path)
        }
        return Bundle.main.url(forAuxiliaryExecutable: "kura-server")
    }

    /// Start the server and return its base URL once it's listening.
    /// `library` is a library folder, or nil for the user's own library.
    func start(library: String?) async throws -> URL {
        stop()
        guard let binary = Self.binary() else {
            throw Failure(message: "Can't find the kura server. Build the app with `just bundle`, or set KURA_BIN.")
        }

        var arguments = ["--bind", "127.0.0.1:0", "--exit-with-stdin"]
        if let library { arguments += ["--library", library] }

        let process = Process()
        let stdin = Pipe()
        let output = Pipe()
        process.executableURL = binary
        process.arguments = arguments
        process.standardInput = stdin
        process.standardOutput = output
        process.standardError = output

        let lines = Self.lines(of: output.fileHandleForReading)
        process.terminationHandler = { [weak self] finished in
            let status = finished.terminationStatus
            Task { @MainActor in self?.exited(finished, status: status) }
        }
        try process.run()
        self.process = process
        self.stdin = stdin

        // Wait for "kura-server listening on http://127.0.0.1:<port>".
        var log: [String] = []
        for await line in lines {
            if let range = line.range(of: "listening on "), let url = URL(string: String(line[range.upperBound...])) {
                return url
            }
            log.append(line)
        }
        throw Failure(message: log.suffix(5).joined(separator: "\n").nonEmpty ?? "The kura server exited at startup.")
    }

    func stop() {
        guard let process else { return }
        self.process = nil
        stdin = nil  // closing stdin is the polite way to stop it
        if process.isRunning { process.terminate() }
    }

    private func exited(_ finished: Process, status: Int32) {
        guard finished === process else { return }  // stopped on purpose, or an older run
        process = nil
        stdin = nil
        onUnexpectedExit?("The kura server stopped (exit status \(status)).")
    }

    /// Every line the server prints, echoed to our own stdout for debugging.
    /// Buffers only the newest lines: after startup nobody reads the stream.
    private static func lines(of handle: FileHandle) -> AsyncStream<String> {
        let (stream, continuation) = AsyncStream.makeStream(of: String.self, bufferingPolicy: .bufferingNewest(50))
        Task.detached {
            do {
                for try await line in handle.bytes.lines {
                    print("[kura] \(line)")
                    continuation.yield(line)
                }
            } catch {}
            continuation.finish()
        }
        return stream
    }
}

private extension String {
    var nonEmpty: String? { isEmpty ? nil : self }
}
