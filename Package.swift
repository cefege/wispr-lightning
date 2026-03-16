// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "WisprLite",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(
            name: "WisprLite",
            path: "Sources/WisprLite",
            resources: [.copy("../../Resources/Info.plist")],
            linkerSettings: [
                .linkedLibrary("sqlite3"),
                .linkedFramework("AVFoundation"),
                .linkedFramework("CoreAudio"),
                .linkedFramework("AudioToolbox"),
            ]
        )
    ]
)
