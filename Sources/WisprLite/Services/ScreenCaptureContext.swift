import AppKit
import Vision

enum ScreenCaptureContext {
    static func captureOCRContext() -> [String] {
        // 1. Find frontmost app's window
        guard let windowList = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] else {
            wLog("ScreenCaptureContext: Failed to get window list")
            return []
        }

        guard let frontApp = NSWorkspace.shared.frontmostApplication else {
            wLog("ScreenCaptureContext: No frontmost application")
            return []
        }

        let frontPID = frontApp.processIdentifier

        // Find the first on-screen window belonging to the frontmost app
        guard let windowInfo = windowList.first(where: {
            ($0[kCGWindowOwnerPID as String] as? Int32) == frontPID &&
            ($0[kCGWindowLayer as String] as? Int) == 0
        }),
        let windowID = windowInfo[kCGWindowNumber as String] as? CGWindowID else {
            wLog("ScreenCaptureContext: No window found for frontmost app")
            return []
        }

        // 2. Capture that window
        guard let cgImage = CGWindowListCreateImage(
            .null,
            .optionIncludingWindow,
            windowID,
            [.boundsIgnoreFraming]
        ) else {
            wLog("ScreenCaptureContext: Screen capture returned nil — likely missing Screen Recording permission")
            return []
        }

        // 3. Run Vision OCR
        let requestHandler = VNImageRequestHandler(cgImage: cgImage, options: [:])
        let textRequest = VNRecognizeTextRequest()
        textRequest.recognitionLevel = .fast
        textRequest.usesLanguageCorrection = false

        do {
            try requestHandler.perform([textRequest])
        } catch {
            wLog("ScreenCaptureContext: Vision OCR failed: \(error.localizedDescription)")
            return []
        }

        guard let observations = textRequest.results else {
            return []
        }

        // 4. Return top candidates, capped at 50 lines
        var lines: [String] = []
        for observation in observations {
            if let topCandidate = observation.topCandidates(1).first {
                lines.append(topCandidate.string)
            }
            if lines.count >= 50 { break }
        }

        return lines
    }
}
