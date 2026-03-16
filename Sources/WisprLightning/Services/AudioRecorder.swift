import AVFoundation
import CoreAudio

class AudioRecorder {
    private let settings: AppSettings
    private var audioEngine: AVAudioEngine?
    private var packets: [Data] = []
    private let lock = NSLock()
    private(set) var isRecording = false

    init(settings: AppSettings) {
        self.settings = settings
    }

    func start() {
        packets = []
        isRecording = true

        let engine = AVAudioEngine()
        self.audioEngine = engine

        // Select input device if specified
        if let deviceUID = settings.micDeviceUID {
            setInputDevice(uid: deviceUID)
        }

        let inputNode = engine.inputNode
        let hwFormat = inputNode.inputFormat(forBus: 0)

        // Target format: 16kHz mono Int16
        guard let targetFormat = AVAudioFormat(
            commonFormat: .pcmFormatInt16,
            sampleRate: Double(Constants.sampleRate),
            channels: 1,
            interleaved: true
        ) else {
            NSLog("Wispr Lightning: Failed to create target audio format")
            return
        }

        // Install tap at hardware format, then convert
        let bufferSize = AVAudioFrameCount(Constants.chunkSamples)
        inputNode.installTap(onBus: 0, bufferSize: bufferSize, format: hwFormat) { [weak self] buffer, _ in
            guard let self = self, self.isRecording else { return }
            self.processBuffer(buffer, from: hwFormat, to: targetFormat)
        }

        do {
            try engine.start()
            NSLog("Wispr Lightning: Audio engine started (input: %@, rate: %.0f Hz)",
                  settings.micDeviceName ?? "system default", hwFormat.sampleRate)
        } catch {
            NSLog("Wispr Lightning: Failed to start audio engine: %@", error.localizedDescription)
            isRecording = false
        }
    }

    func stop() -> [Data] {
        isRecording = false
        audioEngine?.inputNode.removeTap(onBus: 0)
        audioEngine?.stop()
        audioEngine = nil

        lock.lock()
        let result = packets
        lock.unlock()

        NSLog("Wispr Lightning: Recording stopped — %d packets (%.1fs)",
              result.count, Double(result.count) * Double(Constants.chunkDurationMs) / 1000.0)
        return result
    }

    private func processBuffer(_ buffer: AVAudioPCMBuffer, from sourceFormat: AVAudioFormat, to targetFormat: AVAudioFormat) {
        guard let converter = AVAudioConverter(from: sourceFormat, to: targetFormat) else { return }

        let ratio = targetFormat.sampleRate / sourceFormat.sampleRate
        let outputFrameCapacity = AVAudioFrameCount(Double(buffer.frameLength) * ratio)
        guard let outputBuffer = AVAudioPCMBuffer(pcmFormat: targetFormat, frameCapacity: outputFrameCapacity) else { return }

        var error: NSError?
        var consumed = false
        converter.convert(to: outputBuffer, error: &error) { _, outStatus in
            if consumed {
                outStatus.pointee = .noDataNow
                return nil
            }
            consumed = true
            outStatus.pointee = .haveData
            return buffer
        }

        guard error == nil, outputBuffer.frameLength > 0 else { return }

        // Extract Int16 samples and split into 640-sample (40ms) chunks
        let int16Ptr = outputBuffer.int16ChannelData![0]
        let totalSamples = Int(outputBuffer.frameLength)
        let chunkSize = Constants.chunkSamples

        var offset = 0
        while offset + chunkSize <= totalSamples {
            let data = Data(bytes: int16Ptr.advanced(by: offset), count: chunkSize * 2)
            lock.lock()
            packets.append(data)
            lock.unlock()
            offset += chunkSize
        }
    }

    func cleanup() {
        audioEngine?.inputNode.removeTap(onBus: 0)
        audioEngine?.stop()
        audioEngine = nil
    }

    private func setInputDevice(uid: String) {
        // Find device ID matching the UID from our device list
        let devices = AudioRecorder.listInputDevices()
        guard devices.contains(where: { $0.uid == uid }) else { return }

        // Get all audio devices and find the one with matching UID
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDevices,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var devicesSize: UInt32 = 0
        AudioObjectGetPropertyDataSize(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &devicesSize)
        let count = Int(devicesSize) / MemoryLayout<AudioDeviceID>.size
        var deviceIDs = [AudioDeviceID](repeating: 0, count: count)
        AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &devicesSize, &deviceIDs)

        for id in deviceIDs {
            var uidAddress = AudioObjectPropertyAddress(
                mSelector: kAudioDevicePropertyDeviceUID,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain
            )
            var deviceUID: CFString = "" as CFString
            var uidSize = UInt32(MemoryLayout<CFString>.size)
            AudioObjectGetPropertyData(id, &uidAddress, 0, nil, &uidSize, &deviceUID)

            if (deviceUID as String) == uid {
                var mutableID = id
                var inputAddress = AudioObjectPropertyAddress(
                    mSelector: kAudioHardwarePropertyDefaultInputDevice,
                    mScope: kAudioObjectPropertyScopeGlobal,
                    mElement: kAudioObjectPropertyElementMain
                )
                AudioObjectSetPropertyData(
                    AudioObjectID(kAudioObjectSystemObject),
                    &inputAddress, 0, nil,
                    UInt32(MemoryLayout<AudioDeviceID>.size),
                    &mutableID
                )
                return
            }
        }
    }

    static func listInputDevices() -> [(uid: String, name: String)] {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDevices,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )

        var size: UInt32 = 0
        AudioObjectGetPropertyDataSize(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size)

        let count = Int(size) / MemoryLayout<AudioDeviceID>.size
        var deviceIDs = [AudioDeviceID](repeating: 0, count: count)
        AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &deviceIDs)

        var results: [(uid: String, name: String)] = []

        for id in deviceIDs {
            // Check if device has input channels
            var inputAddress = AudioObjectPropertyAddress(
                mSelector: kAudioDevicePropertyStreamConfiguration,
                mScope: kAudioDevicePropertyScopeInput,
                mElement: kAudioObjectPropertyElementMain
            )
            var bufferListSize: UInt32 = 0
            AudioObjectGetPropertyDataSize(id, &inputAddress, 0, nil, &bufferListSize)

            let bufferListPtr = UnsafeMutablePointer<AudioBufferList>.allocate(capacity: 1)
            defer { bufferListPtr.deallocate() }
            AudioObjectGetPropertyData(id, &inputAddress, 0, nil, &bufferListSize, bufferListPtr)

            let bufferList = bufferListPtr.pointee
            guard bufferList.mNumberBuffers > 0, bufferList.mBuffers.mNumberChannels > 0 else { continue }

            // Get device UID
            var uidAddress = AudioObjectPropertyAddress(
                mSelector: kAudioDevicePropertyDeviceUID,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain
            )
            var uid: CFString = "" as CFString
            var uidSize = UInt32(MemoryLayout<CFString>.size)
            AudioObjectGetPropertyData(id, &uidAddress, 0, nil, &uidSize, &uid)

            // Get device name
            var nameAddress = AudioObjectPropertyAddress(
                mSelector: kAudioDevicePropertyDeviceNameCFString,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain
            )
            var name: CFString = "" as CFString
            var nameSize = UInt32(MemoryLayout<CFString>.size)
            AudioObjectGetPropertyData(id, &nameAddress, 0, nil, &nameSize, &name)

            results.append((uid: uid as String, name: name as String))
        }

        return results
    }
}
