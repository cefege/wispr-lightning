import Foundation

/// Placeholders substituted for the two fields that change on every run: `Session.sessionId`
/// is a fresh `UUID()` per process launch and `transcriptUUID` a fresh `UUID()` per dictation.
enum Placeholder {
    static let sessionId = "<SESSION_ID>"
    static let transcriptUUID = "<TRANSCRIPT_UUID>"
    static let token = "<TOKEN>"
}

final class FixtureGenerator {
    private let repoRoot: URL
    private let tree: FixtureTree
    private let sandboxHome: URL

    init(repoRoot: URL, outputRoot: URL, sandboxHome: URL) throws {
        self.repoRoot = repoRoot
        self.sandboxHome = sandboxHome
        self.tree = try FixtureTree(root: outputRoot)
    }

    func run() throws -> FixtureTree {
        let database = try FixtureDatabase.build(sandboxHome: sandboxHome)
        try emitDatabase(database)
        try emitSettings()
        try emitAuthFrames(database: database)
        dictionaryConnection?.manager.close()
        dictionaryConnection = nil
        let packets = SyntheticAudio.generate()
        try emitPcm(packets)
        try emitAppendFrames(packets)
        try emitAscii85()
        try emitPolish()
        try emitProvenance()
        try emitManifest()
        return tree
    }

    // MARK: - db/

    private func emitDatabase(_ database: FixtureDatabase.Built) throws {
        try tree.write(database.schemaSQL, to: "db/schema.sql")

        let destination = tree.url(for: "db/populated.db")
        try FileManager.default.createDirectory(
            at: destination.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        try FileManager.default.copyItem(at: database.databaseURL, to: destination)
        try tree.adopt("db/populated.db")

        try tree.writeJSON([
            "database": "db/populated.db",
            "schema": "db/schema.sql",
            "row_counts": database.rowCounts,
            "expected_reads": [
                "get_vocabulary_phrases": database.vocabularyPhrases,
                "get_replacements": database.replacements,
                "get_snippets": database.snippets
            ]
        ], to: "db/expected.json")
    }

    // MARK: - settings/

    private func emitSettings() throws {
        try tree.writeJSON(try Self.encodable(AppSettings()), to: "settings/default.json")
        try tree.writeJSON(try Self.encodable(Self.fullySetSettings()), to: "settings/full.json")
    }

    /// Round-trips through the reference `Codable` conformance, so the key names in the
    /// fixture are exactly the ones the shipping app reads and writes.
    private static func encodable(_ settings: AppSettings) throws -> Any {
        let data = try JSONEncoder().encode(settings)
        return try JSONSerialization.jsonObject(with: data)
    }

    /// Every stored property of `AppSettings` moved off its default, including both
    /// optionals (which default to `nil` and are therefore absent from `default.json`).
    /// `activePolishInstructions` is computed and correctly does not appear.
    private static func fullySetSettings() -> AppSettings {
        let s = AppSettings()
        s.hotkeyKeyCode = 62
        s.hotkeyLabel = "Right Control"
        s.hotkeyKeyCodes = [55, 58]
        s.hotkeyLabels = ["Left Command", "Left Option"]
        s.micDeviceUID = "AppleUSBAudioEngine:Blue:Yeti Stereo Microphone:1"
        s.micDeviceName = "Yeti Stereo Microphone"
        s.keepMicrophoneActive = true
        s.languages = ["en", "fr", "de"]
        s.launchAtLogin = true
        s.showInDock = true
        s.enableSounds = false
        s.muteMusic = true
        s.aiFormatting = false
        s.autoCleanupLevel = "heavy"
        s.commandModeEnabled = false
        s.useScreenContext = true
        s.useAccessibilityContext = false
        s.shareUsageData = true
        s.styleDetectionEnabled = false
        s.personalizationStyles = [
            "work": "formal",
            "email": "concise",
            "personal": "casual",
            "other": "verbose"
        ]
        s.hyperlinkOn = true
        s.autoLearnWords = false
        s.polishEnabled = true
        // Every default flipped: the five true-by-default become false and vice versa.
        s.polishInstructions = [
            "Make more concise": false,
            "Reword for clarity": false,
            "Maintain your tone": false,
            "Reorder for readability": false,
            "Add structure for readability": false,
            "Clarify main point": true,
            "Refine phrasing for impact": true
        ]
        s.autoPolish = true
        s.polishHotkeyKeyCodes = [61]
        s.polishHotkeyLabels = ["Right Option"]
        s.emailAutoSignature = true
        s.emailSignatureOption = "spoken_with_lightning"
        s.creatorMode = true
        s.selectedSoundPack = "subtle"
        s.verboseLogging = true
        s.hotkeyPaused = true
        s.naturalModeEnabled = true
        s.naturalModeSpeed = "expert"
        return s
    }

    // MARK: - auth/

    private struct AuthCase {
        let name: String
        let purpose: String
        var appInfo: [String: String] = AuthFixtures.defaultAppInfo
        var axContext: [String] = []
        var ocrContext: [String] = []
        var withDictionary = false
        var clearToken = false
        var mutate: (AppSettings) -> Void = { _ in }
    }

    private enum AuthFixtures {
        static let defaultAppInfo = [
            "name": "Notes", "bundle_id": "com.apple.Notes", "type": "other", "url": ""
        ]
        static let axContext = ["Dear Alice,", "\n\nThanks for the "]
        static let ocrContext = ["Inbox — 3 unread", "Compose", "Send"]
        static let accessToken = "fixture-access-token"
    }

    private func emitAuthFrames(database: FixtureDatabase.Built) throws {
        let cases: [AuthCase] = [
            AuthCase(name: "baseline-defaults",
                     purpose: "Stock AppSettings, no contexts, no dictionary — the reference frame every other case is a delta from."),
            AuthCase(name: "formatting-off",
                     purpose: "aiFormatting=false collapses pipeline to [\"transcribe\"].",
                     mutate: { $0.aiFormatting = false }),
            AuthCase(name: "style-detection-off",
                     purpose: "styleDetectionEnabled=false sends personalization_style_settings as {} even though personalizationStyles is populated.",
                     mutate: { $0.styleDetectionEnabled = false }),
            AuthCase(name: "style-detection-on-custom",
                     purpose: "styleDetectionEnabled=true forwards personalizationStyles verbatim.",
                     mutate: { $0.personalizationStyles = ["work": "formal", "email": "concise", "personal": "casual", "other": "verbose"] }),
            AuthCase(name: "creator-mode-on",
                     purpose: "creatorMode=true sets job_selectors to [\"creator\"].",
                     mutate: { $0.creatorMode = true }),
            AuthCase(name: "command-mode-off",
                     purpose: "commandModeEnabled=false flips command_mode.",
                     mutate: { $0.commandModeEnabled = false }),
            AuthCase(name: "hyperlink-on",
                     purpose: "hyperlinkOn=true flips hyperlink_on.",
                     mutate: { $0.hyperlinkOn = true }),
            AuthCase(name: "cleanup-level-none",
                     purpose: "cleanup_level is passed through as a raw string, not an enum.",
                     mutate: { $0.autoCleanupLevel = "none" }),
            AuthCase(name: "ax-context-populated",
                     purpose: "Non-empty ax_context — the only thing that makes prefix_is_written true.",
                     axContext: AuthFixtures.axContext),
            AuthCase(name: "ocr-context-populated",
                     purpose: "Non-empty ocr_context does NOT set prefix_is_written; only ax_context does.",
                     ocrContext: AuthFixtures.ocrContext),
            AuthCase(name: "languages-single",
                     purpose: "Default single-language list.",
                     mutate: { $0.languages = ["en"] }),
            AuthCase(name: "languages-multiple",
                     purpose: "language is an array and keeps the user's order, not sorted.",
                     mutate: { $0.languages = ["en", "es", "ja"] }),
            AuthCase(name: "dictionary-empty",
                     purpose: "No DictionaryStore attached: context [], replacements {}, snippets {}.",
                     withDictionary: false),
            AuthCase(name: "dictionary-populated",
                     purpose: "DictionaryStore attached: 50-phrase LIMIT, deleted rows excluded, and snippets wrapped as single-element arrays.",
                     withDictionary: true),
            AuthCase(name: "app-type-other",
                     purpose: "AppInfoDetector type \"other\".",
                     appInfo: ["name": "Notes", "bundle_id": "com.apple.Notes", "type": "other", "url": ""]),
            AuthCase(name: "app-type-messaging",
                     purpose: "AppInfoDetector type \"messaging\".",
                     appInfo: ["name": "Slack", "bundle_id": "com.tinyspeck.slackmacgap", "type": "messaging", "url": ""]),
            AuthCase(name: "app-type-email",
                     purpose: "AppInfoDetector type \"email\".",
                     appInfo: ["name": "Mail", "bundle_id": "com.apple.mail", "type": "email", "url": ""]),
            AuthCase(name: "app-type-ai",
                     purpose: "AppInfoDetector type \"ai\", carrying a url — the one context field macOS always leaves empty.",
                     appInfo: ["name": "ChatGPT", "bundle_id": "com.openai.chat", "type": "ai", "url": "https://chatgpt.com/c/1"]),
            AuthCase(name: "app-type-uppercase-normalised",
                     purpose: "appInfo[\"type\"] is lowercased before it reaches both `app` and `context.app.type`.",
                     appInfo: ["name": "Slack", "bundle_id": "com.tinyspeck.slackmacgap", "type": "MESSAGING", "url": ""]),
            AuthCase(name: "app-info-non-ascii",
                     purpose: "Non-ASCII app name: JSON must carry raw UTF-8, not \\u escapes.",
                     appInfo: ["name": "Notes — Café", "bundle_id": "com.apple.Notes", "type": "other", "url": ""]),
            AuthCase(name: "everything-on",
                     purpose: "All flags on, both contexts, dictionary attached, three languages.",
                     appInfo: ["name": "Slack", "bundle_id": "com.tinyspeck.slackmacgap", "type": "messaging", "url": ""],
                     axContext: AuthFixtures.axContext,
                     ocrContext: AuthFixtures.ocrContext,
                     withDictionary: true,
                     mutate: {
                         $0.aiFormatting = true
                         $0.styleDetectionEnabled = true
                         $0.personalizationStyles = ["work": "formal", "email": "concise", "personal": "casual", "other": "verbose"]
                         $0.creatorMode = true
                         $0.commandModeEnabled = true
                         $0.hyperlinkOn = true
                         $0.autoCleanupLevel = "heavy"
                         $0.languages = ["en", "fr", "de"]
                     }),
            AuthCase(name: "everything-off-no-token",
                     purpose: "All flags off and Session.accessToken nil, which serialises as \"\" rather than being omitted.",
                     clearToken: true,
                     mutate: {
                         $0.aiFormatting = false
                         $0.styleDetectionEnabled = false
                         $0.creatorMode = false
                         $0.commandModeEnabled = false
                         $0.hyperlinkOn = false
                         $0.autoCleanupLevel = "none"
                     })
        ]

        for authCase in cases {
            let settings = AppSettings()
            authCase.mutate(settings)

            let session = Session()
            session.accessToken = authCase.clearToken ? nil : AuthFixtures.accessToken
            session.userFirstName = "Mike"
            session.userLastName = "Chen"

            let store: DictionaryStore? = authCase.withDictionary ? try loadedDictionaryStore() : nil

            var frame = ProtocolFrames.authMessage(
                session: session,
                settings: settings,
                dictionaryStore: store,
                appInfo: authCase.appInfo,
                ocrContext: authCase.ocrContext,
                axContext: authCase.axContext,
                transcriptUUID: UUID().uuidString
            )
            frame["metadata"] = Self.normalisedMetadata(frame["metadata"])

            try tree.writeJSON(frame, to: "auth/\(authCase.name).json")

            // The frame alone is not testable — the port needs the inputs that produced it.
            let sessionInput: [String: Any] = [
                "access_token": authCase.clearToken ? NSNull() : AuthFixtures.accessToken,
                "user_first_name": "Mike",
                "user_last_name": "Chen"
            ]
            let dictionaryInput: Any = authCase.withDictionary
                ? [
                    "context": database.vocabularyPhrases,
                    "replacements": database.replacements,
                    "snippets": database.snippets
                  ]
                : NSNull()
            try tree.writeJSON([
                "purpose": authCase.purpose,
                "settings": try Self.encodable(settings),
                "session": sessionInput,
                "app_info": authCase.appInfo,
                "ax_context": authCase.axContext,
                "ocr_context": authCase.ocrContext,
                "dictionary": dictionaryInput
            ], to: "auth/\(authCase.name).input.json")
        }
    }

    /// A `DictionaryStore` over the populated fixture database, so the dictionary fields
    /// in the auth frames are produced by the shipping queries rather than hand-written.
    /// Opened once and reused: `DatabaseManager` has no way to hand back an existing
    /// connection, and reopening per case would leave a handle open for each one.
    private var dictionaryConnection: (manager: DatabaseManager, store: DictionaryStore)?

    private func loadedDictionaryStore() throws -> DictionaryStore {
        if let existing = dictionaryConnection { return existing.store }
        let manager = DatabaseManager()
        guard manager.db != nil else { throw FixtureError("could not reopen the fixture database") }
        let store = DictionaryStore(dbManager: manager)
        dictionaryConnection = (manager, store)
        return store
    }

    private static func normalisedMetadata(_ metadata: Any?) -> [String: Any] {
        var dict = (metadata as? [String: Any]) ?? [:]
        dict["session_id"] = Placeholder.sessionId
        dict["transcript_entity_uuid"] = Placeholder.transcriptUUID
        return dict
    }

    // MARK: - pcm/

    private func emitPcm(_ packets: [Data]) throws {
        var pcm = Data(capacity: packets.count * SyntheticAudio.bytesPerPacket)
        for packet in packets { pcm.append(packet) }
        try tree.write(pcm, to: "pcm/input-1001.pcm")
        try tree.write(SyntheticAudio.wavWrap(packets), to: "pcm/input-1001.wav")

        let volumes = packets.map(ProtocolFrames.packetVolume)
        try tree.writeJSON([
            "sample_rate": Constants.sampleRate,
            "channels": Constants.channels,
            "chunk_duration_ms": Constants.chunkDurationMs,
            "samples_per_packet": SyntheticAudio.samplesPerPacket,
            "bytes_per_packet": SyntheticAudio.bytesPerPacket,
            "packet_count": packets.count,
            "pcm_file": "pcm/input-1001.pcm",
            "pcm_sha256": Digest.hex(pcm),
            "wav_file": "pcm/input-1001.wav",
            "wav_header_length": 44,
            "packets": packets.indices.map { ["index": $0, "volume": volumes[$0]] }
        ], to: "pcm/packets.json")
    }

    // MARK: - append/

    private func emitAppendFrames(_ packets: [Data]) throws {
        let encoded = packets.map(ProtocolFrames.ascii85Encode)
        let volumes = packets.map(ProtocolFrames.packetVolume)
        let total = packets.count

        var offset = 0
        var positions: [Int] = []
        while offset < total {
            let frame = ProtocolFrames.appendMessage(
                encodedPackets: encoded,
                volumes: volumes,
                offset: offset,
                totalPackets: total
            )
            try tree.writeJSON(frame, to: String(format: "append/position-%04d.json", offset))
            positions.append(offset)
            offset += ProtocolFrames.chunkSize
        }

        try tree.writeJSON(ProtocolFrames.commitMessage(totalPackets: total), to: "append/commit.json")

        try tree.writeJSON([
            "source_pcm": "pcm/input-1001.pcm",
            "total_packets": total,
            "chunk_size": ProtocolFrames.chunkSize,
            "positions": positions,
            "chunk_count": positions.count,
            "final_chunk_packets": total - positions[positions.count - 1]
        ], to: "append/expected.json")
    }

    // MARK: - ascii85/

    private func emitAscii85() throws {
        var rng = Lcg(seed: 0x4153_4349_3835_0001) // "ASCII85" + version

        struct Vector {
            let name: String
            let purpose: String
            let input: Data
        }

        let vectors: [Vector] = [
            Vector(name: "empty",
                   purpose: "Zero-length input encodes to the empty string. Both files are intentionally 0 bytes.",
                   input: Data()),
            Vector(name: "aligned-4",
                   purpose: "Exactly one full non-zero group: 5 characters, no tail.",
                   input: Data([0xDE, 0xAD, 0xBE, 0xEF])),
            Vector(name: "tail-1",
                   purpose: "4 + 1 bytes: the 1-byte tail emits 2 characters.",
                   input: Data([0xDE, 0xAD, 0xBE, 0xEF, 0x4D])),
            Vector(name: "tail-2",
                   purpose: "4 + 2 bytes: the 2-byte tail emits 3 characters.",
                   input: Data([0xDE, 0xAD, 0xBE, 0xEF, 0x4D, 0x69])),
            Vector(name: "tail-3",
                   purpose: "4 + 3 bytes: the 3-byte tail emits 4 characters.",
                   input: Data([0xDE, 0xAD, 0xBE, 0xEF, 0x4D, 0x69, 0x6B])),
            Vector(name: "zero-tail-after-data",
                   purpose: "A non-zero group followed by an all-zero 2-byte tail: the tail must be \"!!!\", never \"z\".",
                   input: Data([0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x00])),
            Vector(name: "zeros-1280",
                   purpose: "A silent 1280-byte packet: 320 aligned zero groups, each collapsing to \"z\".",
                   input: Data(repeating: 0x00, count: 1280)),
            Vector(name: "zeros-1282",
                   purpose: "1280 zero bytes plus a 2-byte zero tail: 320 \"z\" then \"!!!\" — the same byte value, two encodings, decided purely by alignment.",
                   input: Data(repeating: 0x00, count: 1282)),
            Vector(name: "full-scale-1280",
                   purpose: "A 1280-byte 0xFF packet: the top of the value range, where the base-85 digits saturate.",
                   input: Data(repeating: 0xFF, count: 1280)),
            Vector(name: "random-1024",
                   purpose: "1024 deterministic pseudo-random bytes: broad coverage of the group encoder.",
                   input: rng.bytes(1024))
        ]

        var manifest: [[String: Any]] = []
        for vector in vectors {
            let encoded = ProtocolFrames.ascii85Encode(vector.input)
            try tree.write(vector.input, to: "ascii85/\(vector.name).bin")
            try tree.write(encoded, to: "ascii85/\(vector.name).txt")
            manifest.append([
                "name": vector.name,
                "purpose": vector.purpose,
                "input": "ascii85/\(vector.name).bin",
                "expected": "ascii85/\(vector.name).txt",
                "input_bytes": vector.input.count,
                "expected_bytes": encoded.utf8.count
            ])
        }

        try tree.writeJSON([
            "note": "Each .txt is the exact encoder output with no trailing newline, so it can be compared byte-for-byte.",
            "vectors": manifest
        ], to: "ascii85/manifest.json")
    }

    // MARK: - polish/

    private func emitPolish() throws {
        let session = Session()
        // expiresAt stays 0, which `Session.isValid` treats as "no expiry known" and
        // accepts — this keeps the fixture independent of the wall clock.
        session.accessToken = "fixture-access-token"

        let settings = AppSettings()
        settings.polishEnabled = true

        let instructions = [
            "Add structure for readability",
            "Maintain your tone",
            "Make more concise",
            "Reorder for readability",
            "Reword for clarity"
        ]
        let text = "so basically what i wanted to say is that the thing is broken"

        let captured = try PolishCapture.run(
            session: session,
            settings: settings,
            text: text,
            instructions: instructions
        )

        guard let body = try JSONSerialization.jsonObject(with: captured.body) as? [String: Any] else {
            throw FixtureError("polish body was not a JSON object")
        }
        try tree.writeJSON(body, to: "polish/request.json")

        // `Content-Length` is added by the URL loading system, not by PolishService; it is
        // reported separately so the port is not held to reproducing transport behavior.
        let clientSet = ["Authorization", "Cache-Control", "Content-Type"]
        var headers: [String: String] = [:]
        var transport: [String: String] = [:]
        for (name, value) in captured.headers {
            let redacted = name.caseInsensitiveCompare("Authorization") == .orderedSame
                ? Placeholder.token
                : value
            if clientSet.contains(where: { $0.caseInsensitiveCompare(name) == .orderedSame }) {
                headers[name] = redacted
            } else {
                transport[name] = redacted
            }
        }

        try tree.writeJSON([
            "method": captured.method,
            "url": captured.url,
            "client_set_headers": headers,
            "transport_added_headers": transport,
            "body_bytes": captured.body.count,
            "notes": [
                "Authorization carries the raw access token with no \"Bearer \" prefix; redacted here to \(Placeholder.token).",
                "Captured from the real PolishService through a registered URLProtocol, not transcribed.",
                "body_bytes is the length of the client's own serialisation, whose key order is unspecified; request.json is the same object re-serialised with sorted keys."
            ]
        ], to: "polish/headers.json")

        try tree.writeJSON([
            "instructions": instructions,
            "selected_text": text,
            "note": "PolishService turns the instruction list into a {label: true} map; the list order never reaches the wire."
        ], to: "polish/request.input.json")
    }

    // MARK: - provenance / manifest

    private func emitProvenance() throws {
        func digest(_ relativePath: String) throws -> String {
            let url = repoRoot.appendingPathComponent(relativePath)
            guard FileManager.default.fileExists(atPath: url.path) else {
                throw FixtureError("reference source missing: \(relativePath)")
            }
            return try Digest.hexOfFile(at: url)
        }

        let linked = [
            "Sources/WisprLightning/Models/DictionaryEntry.swift",
            "Sources/WisprLightning/Models/NoteEntry.swift",
            "Sources/WisprLightning/Models/Session.swift",
            "Sources/WisprLightning/Models/Settings.swift",
            "Sources/WisprLightning/Models/TranscriptEntry.swift",
            "Sources/WisprLightning/Services/Constants.swift",
            "Sources/WisprLightning/Services/DatabaseManager.swift",
            "Sources/WisprLightning/Services/DictionaryStore.swift",
            "Sources/WisprLightning/Services/HistoryStore.swift",
            "Sources/WisprLightning/Services/NotesStore.swift",
            "Sources/WisprLightning/Services/PolishService.swift",
            "Sources/WisprLightning/Services/PolishStore.swift"
        ]
        let transcribed = [
            "Sources/WisprLightning/Services/TranscriptionClient.swift",
            "Sources/WisprLightning/App/AppDelegate.swift"
        ]

        let linkedSection: [String: Any] = [
            "how": "Compiled into fixturegen through symlinks in tools/fixturegen/Reference/ and executed directly.",
            "files": Dictionary(uniqueKeysWithValues: try linked.map { ($0, try digest($0)) })
        ]
        let transcribedSection: [String: Any] = [
            "how": "Copied by hand into tools/fixturegen/ProtocolFrames.swift and SyntheticAudio.swift because the originals are private and bound to a live socket. A change to these digests means the transcription needs re-checking.",
            "files": Dictionary(uniqueKeysWithValues: try transcribed.map { ($0, try digest($0)) })
        ]

        try tree.writeJSON([
            "regenerate_with": "swift run fixturegen",
            "linked": linkedSection,
            "transcribed": transcribedSection
        ], to: "provenance.json")
    }

    private func emitManifest() throws {
        let files = tree.entries
            .sorted { $0.path < $1.path }
            .map { ["path": $0.path, "bytes": $0.bytes, "sha256": $0.sha256] as [String: Any] }
        try tree.writeJSON([
            "generated_by": "swift run fixturegen",
            "warning": "Generated output. Regenerate rather than hand-edit; MANIFEST.json will disagree with any manual change.",
            "file_count": files.count,
            "total_bytes": tree.totalBytes,
            "files": files
        ], to: "MANIFEST.json")
    }
}
