import Foundation

/// Verbatim transcriptions of the four private routines in
/// `Sources/WisprLightning/Services/TranscriptionClient.swift` that shape the
/// WebSocket protocol.
///
/// Why transcribed and not called: all four live behind `private` on
/// `TranscriptionClient`, and three of them are welded to a live
/// `URLSessionWebSocketTask` aimed at `Constants.wsURL`
/// (`wss://api.wisprflow.ai/llm/ws`). That URL is a `static let` on an `enum`, so
/// there is no seam to point the client at a local server, and `URLProtocol` — which
/// this tool does use to capture the polish request from the real `PolishService` —
/// does not intercept WebSocket tasks. Observing the genuine frames would mean
/// talking to the production backend with a real account token.
///
/// The bodies below are character-for-character copies of the originals, including
/// the `as Any` casts that only exist to type-check the heterogeneous literals. The
/// SHA-256 of the source file is recorded in `tests/fixtures/provenance.json`, so any
/// edit to the reference turns into a visible diff rather than silent drift.
enum ProtocolFrames {
    /// `TranscriptionClient.chunkSize` — max packets per `append` message.
    static let chunkSize = 500

    /// Transcribed from `performTranscription`, lines 123–168.
    ///
    /// `transcriptUUID` and `session.sessionId` are the two nondeterministic fields;
    /// the caller substitutes placeholders after the fact so this stays a faithful
    /// copy of the original expression.
    static func authMessage(
        session: Session,
        settings: AppSettings,
        dictionaryStore: DictionaryStore?,
        appInfo: [String: String],
        ocrContext: [String],
        axContext: [String],
        transcriptUUID: String
    ) -> [String: Any] {
        let appType = (appInfo["type"] ?? "other").lowercased()

        let pipeline = settings.aiFormatting ? ["transcribe", "format"] : ["transcribe"]
        let authMsg: [String: Any] = [
            "type": "auth",
            "access_token": session.accessToken ?? "",
            "app": appType,
            "context": [
                "app": [
                    "name": appInfo["name"] ?? "",
                    "bundle_id": appInfo["bundle_id"] ?? "",
                    "type": appType,
                    "url": appInfo["url"] ?? ""
                ],
                "ax_context": axContext,
                "ocr_context": ocrContext,
                "dictionary_context": (dictionaryStore?.getVocabularyPhrases() ?? []) as Any,
                "dictionary_replacements": (dictionaryStore?.getReplacements() ?? [:]) as Any,
                "dictionary_snippets": (dictionaryStore?.getSnippets() ?? [:]).mapValues { [$0] } as Any,
                "user_first_name": session.userFirstName ?? "",
                "user_last_name": session.userLastName ?? "",
                "textbox_contents": [:] as [String: Any],
                "content_text": "",
                "variable_names": [] as [Any],
                "file_names": [] as [Any]
            ] as [String: Any],
            "personalization_style_settings": settings.styleDetectionEnabled ? settings.personalizationStyles : [:] as [String: String],
            "language": settings.languages,
            "metadata": [
                "session_id": session.sessionId,
                "environment": "PRODUCTION",
                "client_platform": "darwin",
                "client_version": Constants.clientVersion,
                "transcript_entity_uuid": transcriptUUID
            ] as [String: Any],
            "pipeline": pipeline,
            "job_selectors": (settings.creatorMode ? ["creator"] : []) as [Any],
            "cleanup_level": settings.autoCleanupLevel,
            "command_mode": settings.commandModeEnabled,
            "debug_mode": false,
            "use_staging_baseten": false,
            "prefix_is_written": !axContext.isEmpty,
            "hyperlink_on": settings.hyperlinkOn
        ]
        return authMsg
    }

    /// Transcribed from `sendNextChunk`, lines 306–323. `offset`/`totalPackets` keep
    /// the original meaning: `offset` is a packet index, not a byte offset.
    static func appendMessage(
        encodedPackets: [String],
        volumes: [Double],
        offset: Int,
        totalPackets: Int
    ) -> [String: Any] {
        let end = min(offset + Self.chunkSize, totalPackets)
        let isFinal = end >= totalPackets
        let chunkPackets = Array(encodedPackets[offset..<end])
        let chunkVolumes = Array(volumes[offset..<end])

        let appendMsg: [String: Any] = [
            "type": "append",
            "audio_packets": [
                "packets": chunkPackets,
                "volumes": chunkVolumes,
                "packet_duration": Double(Constants.chunkDurationMs) / 1000.0,
                "audio_encoding": "wav",
                "byte_encoding": "ascii85"
            ] as [String: Any],
            "position": offset,
            "final": isFinal
        ]
        return appendMsg
    }

    /// Transcribed from `sendCommitAndReceive`, lines 283–286.
    static func commitMessage(totalPackets: Int) -> [String: Any] {
        let commitMsg: [String: Any] = [
            "type": "commit",
            "total_packets": totalPackets
        ]
        return commitMsg
    }

    /// Transcribed from `prepareAudio`, lines 251–265 — the per-packet RMS volume.
    ///
    /// Every intermediate is exact in a `Double`: the largest possible `sumSquares`
    /// is `640 * 32768^2 ≈ 6.9e11`, well inside 2^53, and `sqrt` is correctly rounded
    /// by IEEE-754, so this reproduces bit-for-bit anywhere.
    static func packetVolume(_ packet: Data) -> Double {
        let sampleCount = packet.count / 2
        var sumSquares: Double = 0
        packet.withUnsafeBytes { rawBuffer in
            let samples = rawBuffer.bindMemory(to: Int16.self)
            for i in 0..<sampleCount {
                let s = Double(samples[i])
                sumSquares += s * s
            }
        }
        let rms = (sumSquares / Double(sampleCount)).squareRoot()
        return (rms / 32768.0 * 10000).rounded() / 10000
    }

    /// Transcribed from `ascii85Encode`, lines 445–485. Classic btoa/Adobe base-85
    /// without the `<~ ~>` framing: `z` for a full all-zero group, and a partial tail
    /// of `n` bytes emitting `n + 1` characters (so an all-zero *tail* is a run of
    /// `!`, never `z`).
    static func ascii85Encode(_ data: Data) -> String {
        let byteCount = data.count
        // Pre-allocate output buffer: each 4-byte group becomes at most 5 bytes
        var output = [UInt8]()
        output.reserveCapacity((byteCount / 4 + 1) * 5)

        data.withUnsafeBytes { rawBuffer in
            let bytes = rawBuffer.bindMemory(to: UInt8.self)
            var i = 0
            while i < byteCount {
                var value: UInt32 = 0
                let remaining = min(4, byteCount - i)
                for j in 0..<4 {
                    value = value << 8
                    if j < remaining {
                        value |= UInt32(bytes[i + j])
                    }
                }

                if remaining == 4 && value == 0 {
                    output.append(0x7A) // 'z'
                } else {
                    var encoded: (UInt8, UInt8, UInt8, UInt8, UInt8) = (0, 0, 0, 0, 0)
                    encoded.4 = UInt8(value % 85) + 33; value /= 85
                    encoded.3 = UInt8(value % 85) + 33; value /= 85
                    encoded.2 = UInt8(value % 85) + 33; value /= 85
                    encoded.1 = UInt8(value % 85) + 33; value /= 85
                    encoded.0 = UInt8(value % 85) + 33
                    let outputCount = remaining < 4 ? remaining + 1 : 5
                    output.append(encoded.0)
                    if outputCount > 1 { output.append(encoded.1) }
                    if outputCount > 2 { output.append(encoded.2) }
                    if outputCount > 3 { output.append(encoded.3) }
                    if outputCount > 4 { output.append(encoded.4) }
                }
                i += 4
            }
        }

        return String(bytes: output, encoding: .ascii) ?? ""
    }
}
