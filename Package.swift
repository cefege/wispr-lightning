// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "WisprLightning",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(
            name: "WisprLightning",
            path: "Sources/WisprLightning",
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
