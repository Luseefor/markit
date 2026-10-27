// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "Markit",
    platforms: [.iOS(.v15)],
    products: [.library(name: "Markit", targets: ["Markit"])],
    targets: [
        .target(name: "CMarkit", publicHeadersPath: "include"),
        .target(name: "Markit", dependencies: ["CMarkit"])
    ]
)
