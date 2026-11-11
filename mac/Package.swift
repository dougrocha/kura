// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "Kura",
    // Liquid Glass (glassEffect, glass button styles) starts at macOS 26.
    platforms: [.macOS(.v26)],
    targets: [
        .executableTarget(name: "Kura", path: "Sources/Kura"),
        .testTarget(name: "KuraTests", dependencies: ["Kura"], path: "Tests/KuraTests"),
    ]
)
