// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "WisprLightning",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(
            name: "WisprLightning",
            path: "Sources/WisprLightning",
            resources: [.copy("../../Resources/Info.plist"), .copy("../../Resources/Sounds"), .copy("../../Resources/AppIcon.icns"), .copy("../../Resources/WisprFlowIcon.png")],
            linkerSettings: [
                .linkedLibrary("sqlite3"),
                .linkedFramework("AVFoundation"),
                .linkedFramework("CoreAudio"),
                .linkedFramework("AudioToolbox"),
                .linkedFramework("Carbon"),
            ]
        ),
        // `fixturegen` (docs/PORT_PLAN.md §5.1) dumps golden protocol fixtures from
        // this reference implementation so the Rust port can be proven byte-equal
        // against it.
        //
        // It needs to *run* the shipping storage and settings code, not a copy of it,
        // but SwiftPM rejects two targets that own the same source file and an
        // `executableTarget` cannot be imported. The alternatives were to carve the
        // shared files out into a library target — which means editing the `import`
        // list of every file in the shipping app to serve a build-time tool, and
        // risking a behavior change in the thing we are supposed to be measuring —
        // or to duplicate them. Neither is acceptable for an oracle.
        //
        // Instead `tools/fixturegen/Reference/` holds symlinks to the dozen reference
        // files the generator needs. SwiftPM follows them and compiles the real code
        // into the tool, while the `WisprLightning` target above stays byte-for-byte
        // untouched. The frames that genuinely cannot be reached this way (they are
        // `private` and welded to a live socket) are transcribed in
        // `tools/fixturegen/ProtocolFrames.swift`, with source digests recorded in
        // `tests/fixtures/provenance.json` so drift shows up as a diff.
        .executableTarget(
            name: "fixturegen",
            path: "tools/fixturegen",
            linkerSettings: [.linkedLibrary("sqlite3")]
        )
    ]
)
