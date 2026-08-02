import Foundation

/// A deterministic stand-in for a real recording: 1001 packets of 16-bit little-endian
/// mono PCM at 16 kHz, matching what `AudioRecorder` hands to `TranscriptionClient`.
///
/// 1001 is chosen to land one packet past the second 500-packet chunk boundary, so the
/// `append` fixtures exercise a final chunk of size 1 rather than a clean multiple.
///
/// The waveform is a 100 Hz integer triangle under a sawtooth envelope, plus LCG dither.
/// No `sin`, no floating point: libm is not bit-identical across platforms and these
/// bytes are committed, so regeneration on another machine must reproduce them exactly.
enum SyntheticAudio {
    static let packetCount = 1001
    static let samplesPerPacket = Constants.chunkSamples   // 640
    static let bytesPerPacket = Constants.chunkSamples * 2 // 1280

    /// Three packets are pinned to exact edge cases so the RMS and ascii85 fixtures
    /// carry their own boundary coverage:
    /// - packet 0 is digital silence → volume `0.0`, and 320 consecutive `z` groups;
    /// - packet 500 is `Int16.max` throughout → `32767/32768*10000` rounds up to
    ///   `10000`, i.e. volume `1.0`, pinning the half-away-from-zero rounding;
    /// - packet 1000 is `Int16.min` throughout → RMS exactly `32768` → volume `1.0`.
    static let silentPacketIndex = 0
    static let positiveFullScalePacketIndex = 500
    static let negativeFullScalePacketIndex = 1000

    static func generate() -> [Data] {
        var rng = Lcg(seed: 0x5749_5350_5200_0001) // "WISPR" + version
        var packets: [Data] = []
        packets.reserveCapacity(packetCount)

        for packetIndex in 0..<packetCount {
            var samples = [Int16](repeating: 0, count: samplesPerPacket)

            switch packetIndex {
            case silentPacketIndex:
                break
            case positiveFullScalePacketIndex:
                for i in 0..<samplesPerPacket { samples[i] = Int16.max }
            case negativeFullScalePacketIndex:
                for i in 0..<samplesPerPacket { samples[i] = Int16.min }
            default:
                // Envelope cycles over 97 packets — coprime with both 500 and 640, so no
                // chunk boundary sees a repeating volume pattern.
                let envelopeNumerator = (packetIndex % 97) + 1
                for i in 0..<samplesPerPacket {
                    let n = packetIndex * samplesPerPacket + i
                    // 160 samples at 16 kHz = one 100 Hz period.
                    let phase = n % 160
                    let triangle = phase < 80 ? phase : 160 - phase   // 0...80
                    let tone = (triangle - 40) * 750                  // -30000...+30000
                    let shaped = tone * envelopeNumerator / 97
                    let dither = Int(Int8(bitPattern: rng.nextByte()))
                    samples[i] = Int16(clamping: shaped + dither)
                }
            }

            var packet = Data(capacity: bytesPerPacket)
            for sample in samples {
                let bits = UInt16(bitPattern: sample)
                packet.append(UInt8(truncatingIfNeeded: bits))
                packet.append(UInt8(truncatingIfNeeded: bits >> 8))
            }
            packets.append(packet)
        }

        return packets
    }

    /// Byte-for-byte the header `AppDelegate.saveAudioToDownloads` writes
    /// (`Sources/WisprLightning/App/AppDelegate.swift`, lines 681–706): 44-byte
    /// canonical RIFF/WAVE, PCM, mono, 16 kHz, 16-bit.
    static func wavWrap(_ packets: [Data]) -> Data {
        let dataSize = UInt32(packets.count * bytesPerPacket)
        var wav = Data(capacity: 44 + Int(dataSize))

        func appendU16(_ d: inout Data, _ v: UInt16) { var le = v.littleEndian; d.append(Data(bytes: &le, count: 2)) }
        func appendU32(_ d: inout Data, _ v: UInt32) { var le = v.littleEndian; d.append(Data(bytes: &le, count: 4)) }

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
