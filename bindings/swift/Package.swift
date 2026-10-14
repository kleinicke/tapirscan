// swift-tools-version:5.9
// Build TapirscanC.xcframework first: python3 scripts/build_apple.py
import PackageDescription

let package = Package(
    name: "Tapirscan",
    platforms: [.iOS(.v15), .macOS(.v12)],
    products: [.library(name: "Tapirscan", targets: ["Tapirscan"])],
    targets: [
        .binaryTarget(name: "TapirscanC", path: "TapirscanC.xcframework"),
        .target(name: "Tapirscan", dependencies: ["TapirscanC"]),
        .testTarget(name: "TapirscanTests", dependencies: ["Tapirscan"]),
    ]
)
