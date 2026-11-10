// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "SymairaEraseMe",
    platforms: [
        .macOS(.v14)
    ],
    dependencies: [
        .package(url: "https://github.com/danieljustus/symaira-corekit.git", exact: "0.20.0"),
    ],
    targets: [
        .executableTarget(
            name: "SymairaEraseMe",
            dependencies: [
                .product(name: "SymairaTheme", package: "symaira-corekit"),
                .product(name: "SymairaToolKit", package: "symaira-corekit"),
                .product(name: "SymairaDaemonKit", package: "symaira-corekit"),
            ],
            path: "Sources/SymairaEraseMe"
        ),
        .testTarget(
            name: "SymairaEraseMeTests",
            dependencies: ["SymairaEraseMe"],
            path: "Tests/SymairaEraseMeTests"
        ),
    ]
)
