// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "AliceKit",
    platforms: [.iOS(.v17), .macOS(.v14)],
    products: [.library(name: "AliceKit", targets: ["AliceKit"])],
    targets: [
        .target(name: "AliceKit"),
        .testTarget(name: "AliceKitTests", dependencies: ["AliceKit"]),
    ]
)
