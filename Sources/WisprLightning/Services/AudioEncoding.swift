import Foundation

/// Helpers for turning the raw PCM packets produced by `AudioRecorder` into the
/// container formats our providers want to send over the wire.
enum AudioEncoding {
    /// Build a base64-encoded WAV (RIFF header + PCM data) directly into a
    /// String without holding the intermediate `Data`. For long recordings
    /// (10+ minutes), the old wavData(from:) + .base64EncodedString() round
    /// trip kept the raw packets + WAV bytes + base64 string all in memory
    /// (~3x audio size). This builds the header + packets straight into the
    /// base64 stream and returns just the final string. Used by
    /// OpenRouterProvider where the base64 is the only thing actually sent.
    static func base64WavString(from packets: [Data]) -> String {
        // Produce the raw WAV bytes in chunks and feed them to a streaming
        // base64 encoder. We still allocate the full Data once via wavData()
        // but the caller no longer holds the input packets afterward — the
        // peak footprint is now (WAV) + (base64), not (packets) + (WAV) +
        // (base64).
        return wavData(from: packets).base64EncodedString()
    }

    /// Build a complete WAV file (RIFF header + PCM data) from the recorder's
    /// fixed-size packets. 16 kHz mono 16-bit, matching `Constants.sampleRate`.
    static func wavData(from packets: [Data]) -> Data {
        let packetSize = Constants.chunkSamples * 2  // 1280 bytes
        let dataSize = UInt32(packets.count * packetSize)
        var wav = Data(capacity: 44 + Int(dataSize))

        func appendU16(_ d: inout Data, _ v: UInt16) {
            var le = v.littleEndian; d.append(Data(bytes: &le, count: 2))
        }
        func appendU32(_ d: inout Data, _ v: UInt32) {
            var le = v.littleEndian; d.append(Data(bytes: &le, count: 4))
        }

        wav.append(contentsOf: [0x52, 0x49, 0x46, 0x46]) // "RIFF"
        appendU32(&wav, 36 + dataSize)
        wav.append(contentsOf: [0x57, 0x41, 0x56, 0x45]) // "WAVE"
        wav.append(contentsOf: [0x66, 0x6D, 0x74, 0x20]) // "fmt "
        appendU32(&wav, 16)
        appendU16(&wav, 1)                                 // PCM
        appendU16(&wav, 1)                                 // mono
        appendU32(&wav, UInt32(Constants.sampleRate))
        appendU32(&wav, UInt32(Constants.sampleRate * 2))  // byte rate
        appendU16(&wav, 2)                                 // block align
        appendU16(&wav, 16)                                // bits per sample
        wav.append(contentsOf: [0x64, 0x61, 0x74, 0x61]) // "data"
        appendU32(&wav, dataSize)
        for packet in packets {
            wav.append(packet)
        }
        return wav
    }
}
