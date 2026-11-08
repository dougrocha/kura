// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "Kura",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(name: "Kura", path: "Sources/Kura")
    ]
)
