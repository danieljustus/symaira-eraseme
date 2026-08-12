// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "SymairaEraseMe",
    platforms: [
        .macOS(.v14)
    ],
    dependencies: [
        .package(url: "https://github.com/danieljustus/symaira-appkit.git", exact: "0.9.2"),
    ],
    targets: [
        .executableTarget(
            name: "SymairaEraseMe",
            dependencies: [
                .product(name: "SymairaTheme", package: "symaira-appkit"),
                .product(name: "SymairaToolKit", package: "symaira-appkit"),
                .product(name: "SymairaDaemonKit", package: "symaira-appkit"),
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
