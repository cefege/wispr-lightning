import AVFoundation
import CoreAudio

class AudioRecorder {
    enum StartResult {
        case started
        case startedWithFallback
        case failed(String)
    }

    private let settings: AppSettings
    private var audioEngine: AVAudioEngine
    private var packets: [Data] = []
    private let packetsLock = NSLock()
    private let cacheLock = NSLock()
    private(set) var isRecording = false
    private var isPrewarmed = false
    private var cachedConverter: AVAudioConverter?
    private var cachedDeviceID: AudioDeviceID?
    private var cachedDeviceUID: String?

    private var engineConfigObserver: NSObjectProtocol?

    init(settings: AppSettings) {
        self.settings = settings
        self.audioEngine = AVAudioEngine()
        installDeviceChangeListener()
        engineConfigObserver = NotificationCenter.default.addObserver(
            forName: .AVAudioEngineConfigurationChange,
            object: audioEngine,
            queue: nil
        ) { [weak self] _ in
            guard let self = self else { return }
            NSLog("Wispr Lightning: AVAudioEngine configuration changed")
            self.invalidateDeviceCache()
            DispatchQueue.main.async {
                NotificationCenter.default.post(name: .audioDevicesChanged, object: nil)
            }
        }
    }

    deinit {
        if let observer = engineConfigObserver {
            NotificationCenter.default.removeObserver(observer)
        }
        removeDeviceChangeListener()
    }

    // MARK: - Device change notifications

    private var deviceListListenerBlock: AudioObjectPropertyListenerBlock?
    private var defaultInputListenerBlock: AudioObjectPropertyListenerBlock?

    private static var deviceListAddress = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyDevices,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )

    private static var defaultInputAddress = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyDefaultInputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )

    private func invalidateDeviceCache() {
        cacheLock.lock()
        cachedDeviceID = nil
        cachedDeviceUID = nil
        cacheLock.unlock()
    }

    private func installDeviceChangeListener() {
        // Distinct block instances: AudioObject*PropertyListenerBlock matches by
        // block identity, so reusing a single block reference for two properties
        // would give us only one registration and a leaked listener at remove time.
        let postNotification: () -> Void = { [weak self] in
            self?.invalidateDeviceCache()
            DispatchQueue.main.async {
                NotificationCenter.default.post(name: .audioDevicesChanged, object: nil)
            }
        }
        let listBlock: AudioObjectPropertyListenerBlock = { _, _ in postNotification() }
        let defaultBlock: AudioObjectPropertyListenerBlock = { _, _ in postNotification() }
        deviceListListenerBlock = listBlock
        defaultInputListenerBlock = defaultBlock
        AudioObjectAddPropertyListenerBlock(
            AudioObjectID(kAudioObjectSystemObject),
            &Self.deviceListAddress, nil, listBlock
        )
        AudioObjectAddPropertyListenerBlock(
            AudioObjectID(kAudioObjectSystemObject),
            &Self.defaultInputAddress, nil, defaultBlock
        )
    }

    private func removeDeviceChangeListener() {
        if let block = deviceListListenerBlock {
            AudioObjectRemovePropertyListenerBlock(
                AudioObjectID(kAudioObjectSystemObject),
                &Self.deviceListAddress, nil, block
            )
            deviceListListenerBlock = nil
        }
        if let block = defaultInputListenerBlock {
            AudioObjectRemovePropertyListenerBlock(
                AudioObjectID(kAudioObjectSystemObject),
                &Self.defaultInputAddress, nil, block
            )
            defaultInputListenerBlock = nil
        }
    }

    @discardableResult
    private func selectConfiguredDevice() -> Bool {
        guard let deviceUID = settings.micDeviceUID else { return true }

        cacheLock.lock()
        let uid = cachedDeviceUID
        let id = cachedDeviceID
        cacheLock.unlock()

        if deviceUID == uid, let cachedID = id {
            if setInputDeviceDirect(deviceID: cachedID) {
                return true
            }
            invalidateDeviceCache()
        }
        if setInputDevice(uid: deviceUID) {
            return true
        }
        NSLog("Wispr Lightning: Requested mic '%@' not available, using system default",
              settings.micDeviceName ?? deviceUID)
        return false
    }

    /// Installs a tap on the audio engine's input node, creates/reuses a format converter,
    /// and starts the engine. Throws on engine start failure.
    private func setupAndStartEngine() throws {
        let inputNode = audioEngine.inputNode
        let hwFormat = inputNode.inputFormat(forBus: 0)
        guard let targetFormat = AVAudioFormat(
            commonFormat: .pcmFormatInt16,
            sampleRate: Double(Constants.sampleRate),
            channels: 1,
            interleaved: true
        ) else {
            throw NSError(domain: "AudioRecorder", code: -1,
                          userInfo: [NSLocalizedDescriptionKey: "Failed to create target audio format"])
        }
        if let c = cachedConverter, c.inputFormat == hwFormat, c.outputFormat == targetFormat {
            // reuse existing converter
        } else {
            cachedConverter = AVAudioConverter(from: hwFormat, to: targetFormat)
        }
        let bufferSize = AVAudioFrameCount(Constants.chunkSamples)
        inputNode.installTap(onBus: 0, bufferSize: bufferSize, format: hwFormat) { [weak self] buffer, _ in
            guard let self = self, self.isRecording else { return }
            self.processBuffer(buffer, from: hwFormat, to: targetFormat)
        }
        try audioEngine.start()
    }

    func prewarm() {
        guard !isPrewarmed, !isRecording else { return }
        selectConfiguredDevice()
        do {
            try setupAndStartEngine()
            isPrewarmed = true
            NSLog("Wispr Lightning: Microphone pre-warmed (input: %@)", settings.micDeviceName ?? "system default")
        } catch {
            NSLog("Wispr Lightning: Failed to pre-warm microphone: %@", error.localizedDescription)
            audioEngine.inputNode.removeTap(onBus: 0)
        }
    }

    func deactivate() {
        guard isPrewarmed, !isRecording else { return }
        audioEngine.inputNode.removeTap(onBus: 0)
        audioEngine.stop()
        isPrewarmed = false
        NSLog("Wispr Lightning: Microphone deactivated")
    }

    @discardableResult
    func start() -> StartResult {
        packets = []
        isRecording = true

        if isPrewarmed {
            if audioEngine.isRunning {
                NSLog("Wispr Lightning: Recording started (prewarmed mic)")
                return .started
            }
            // Engine stopped unexpectedly (e.g. audio device change) — reset and restart below
            audioEngine.inputNode.removeTap(onBus: 0)
            isPrewarmed = false
        }

        let fellBack = !selectConfiguredDevice()

        do {
            try setupAndStartEngine()
            NSLog("Wispr Lightning: Audio engine started (input: %@, rate: %.0f Hz)",
                  settings.micDeviceName ?? "system default",
                  audioEngine.inputNode.inputFormat(forBus: 0).sampleRate)
            return fellBack ? .startedWithFallback : .started
        } catch {
            NSLog("Wispr Lightning: Failed to start audio engine: %@", error.localizedDescription)
            audioEngine.inputNode.removeTap(onBus: 0)
            isRecording = false
            return .failed(error.localizedDescription)
        }
    }

    func stop() -> [Data] {
        isRecording = false

        packetsLock.lock()
        let result = packets
        packetsLock.unlock()

        NSLog("Wispr Lightning: Recording stopped — %d packets (%.1fs)",
              result.count, Double(result.count) * Double(Constants.chunkDurationMs) / 1000.0)

        // Keep engine running — prevents CoreAudio reconfiguration that causes Bluetooth audio dropout
        isPrewarmed = true
        return result
    }

    private func processBuffer(_ buffer: AVAudioPCMBuffer, from sourceFormat: AVAudioFormat, to targetFormat: AVAudioFormat) {
        guard let converter = cachedConverter else { return }

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
            packetsLock.lock()
            packets.append(data)
            packetsLock.unlock()
            offset += chunkSize
        }
    }

    func cleanup() {
        audioEngine.inputNode.removeTap(onBus: 0)
        audioEngine.stop()
        cachedConverter = nil
    }

    @discardableResult
    private func setInputDeviceDirect(deviceID: AudioDeviceID) -> Bool {
        var mutableID = deviceID
        var inputAddress = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDefaultInputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        let status = AudioObjectSetPropertyData(
            AudioObjectID(kAudioObjectSystemObject),
            &inputAddress, 0, nil,
            UInt32(MemoryLayout<AudioDeviceID>.size),
            &mutableID
        )
        if status != noErr {
            NSLog("Wispr Lightning: Failed to set input device %d (OSStatus %d)", deviceID, status)
            return false
        }
        return true
    }

    @discardableResult
    private func setInputDevice(uid: String) -> Bool {
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
                guard setInputDeviceDirect(deviceID: id) else { return false }
                cacheLock.lock()
                cachedDeviceID = id
                cachedDeviceUID = uid
                cacheLock.unlock()
                return true
            }
        }
        return false
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
