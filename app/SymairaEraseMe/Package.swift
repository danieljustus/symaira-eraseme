// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "SymairaEraseMe",
    platforms: [
        .macOS(.v14)
    ],
    dependencies: [
        // ponytail: pin the published shared workspace/resource fix (AppKit #170).
        // Move to an exact AppKit release tag before releasing this GUI.
        .package(
            url: "https://github.com/danieljustus/symaira-appkit.git",
            revision: "50baf937221fd7a8be35f35b107fe5e96fe95132"
        )
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
