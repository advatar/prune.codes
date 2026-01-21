// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "A2UIRuntime",
    platforms: [
        .macOS(.v13)
    ],
    products: [
        .library(
            name: "A2UIRuntime",
            targets: ["A2UIRuntime"]
        )
    ],
    targets: [
        .target(
            name: "A2UIRuntime"
        ),
        .testTarget(
            name: "A2UIRuntimeTests",
            dependencies: ["A2UIRuntime"],
            resources: [
                .process("Fixtures")
            ]
        )
    ]
)
