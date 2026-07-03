// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "SymairaDashboard",
    platforms: [
        .macOS(.v14)
    ],
    targets: [
        .executableTarget(
            name: "SymairaDashboard",
            path: "Sources/SymairaDashboard"
        )
    ]
)
