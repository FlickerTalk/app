// swift-tools-version:5.5
// FlickerTalk's native bridge on iOS (see ../src/lib.rs).

import PackageDescription

let package = Package(
    name: "tauri-plugin-ft-platform",
    platforms: [
        .iOS(.v15),
    ],
    products: [
        .library(
            name: "tauri-plugin-ft-platform",
            type: .static,
            targets: ["tauri-plugin-ft-platform"]),
    ],
    dependencies: [
        .package(name: "Tauri", path: "../.tauri/tauri-api")
    ],
    targets: [
        .target(
            name: "tauri-plugin-ft-platform",
            dependencies: [
                .byName(name: "Tauri")
            ],
            path: "Sources")
    ]
)
