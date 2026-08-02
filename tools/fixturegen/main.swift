import Foundation

// Must come first: the reference storage types latch their on-disk locations off the
// home directory the moment anything touches them, and this generator must never read
// or write the developer's live database. See `SandboxHome`.
let sandboxHome = SandboxHome.activate()
defer { try? FileManager.default.removeItem(at: sandboxHome) }

/// Where the reference implementation lives. Preferring the working directory keeps
/// `swift run fixturegen` working from the package root, while the compile-time path is
/// the fallback for `.build/debug/fixturegen` invoked from somewhere else.
func locateRepoRoot() -> URL? {
    func isRepoRoot(_ url: URL) -> Bool {
        let fm = FileManager.default
        return fm.fileExists(atPath: url.appendingPathComponent("Package.swift").path)
            && fm.fileExists(atPath: url.appendingPathComponent("Sources/WisprLightning").path)
    }

    let cwd = URL(fileURLWithPath: FileManager.default.currentDirectoryPath, isDirectory: true)
    if isRepoRoot(cwd) { return cwd }

    // tools/fixturegen/main.swift -> tools/fixturegen -> tools -> repo root
    var candidate = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()
        .deletingLastPathComponent()
        .deletingLastPathComponent()
    candidate.standardize()
    return isRepoRoot(candidate) ? candidate : nil
}

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data("fixturegen: \(message)\n".utf8))
    exit(1)
}

guard let repoRoot = locateRepoRoot() else {
    fail("could not locate the repository root; run `swift run fixturegen` from the package directory")
}

// `--out <dir>` exists so the determinism check can generate two trees side by side and
// diff them without disturbing the committed fixtures.
var outputRoot = repoRoot.appendingPathComponent("tests/fixtures", isDirectory: true)
var arguments = Array(CommandLine.arguments.dropFirst())
while let argument = arguments.first {
    arguments.removeFirst()
    switch argument {
    case "--out":
        guard let value = arguments.first else { fail("--out requires a directory") }
        arguments.removeFirst()
        outputRoot = URL(fileURLWithPath: value, isDirectory: true)
    default:
        fail("unknown argument \(argument); usage: fixturegen [--out <dir>]")
    }
}

do {
    let generator = try FixtureGenerator(
        repoRoot: repoRoot,
        outputRoot: outputRoot,
        sandboxHome: sandboxHome
    )
    let tree = try generator.run()
    print("fixturegen: wrote \(tree.entries.count) files (\(tree.totalBytes) bytes) to \(outputRoot.path)")
} catch {
    fail("\(error)")
}
