import Foundation

/// Captures the polish request the *real* `PolishService` puts on the wire.
///
/// `PolishService.performPolish` is private, but it is reachable through the public
/// `polish(text:instructions:)` and it goes out over `URLSession.shared` — and a
/// globally registered `URLProtocol` does intercept `URLSession.shared`. So unlike the
/// WebSocket frames, this fixture is observed rather than transcribed: the URL, the
/// method, every header and the exact body bytes come from the shipping code.
enum PolishCapture {
    struct Captured {
        let method: String
        let url: String
        let headers: [String: String]
        let body: Data
    }

    private final class Interceptor: URLProtocol {
        nonisolated(unsafe) static var captured: Captured?

        override class func canInit(with request: URLRequest) -> Bool {
            request.url?.host == "api.wisprflow.ai"
        }

        override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

        override func startLoading() {
            // `URLSession` hands a body set via `httpBody` to a URLProtocol as a stream,
            // so both spellings have to be handled to get the bytes the client sent.
            var body = request.httpBody ?? Data()
            if body.isEmpty, let stream = request.httpBodyStream {
                stream.open()
                var buffer = [UInt8](repeating: 0, count: 64 * 1024)
                while stream.hasBytesAvailable {
                    let read = stream.read(&buffer, maxLength: buffer.count)
                    if read <= 0 { break }
                    body.append(contentsOf: buffer[0..<read])
                }
                stream.close()
            }

            Interceptor.captured = Captured(
                method: request.httpMethod ?? "",
                url: request.url?.absoluteString ?? "",
                headers: request.allHTTPHeaderFields ?? [:],
                body: body
            )

            // A minimal well-formed reply so `PolishService` runs its success path to
            // completion instead of leaving the semaphore hanging. If the response cannot
            // be built the request is failed rather than left dangling, so `run` reports a
            // timeout instead of hanging the generator.
            guard let url = request.url,
                  let response = HTTPURLResponse(
                      url: url,
                      statusCode: 200,
                      httpVersion: "HTTP/1.1",
                      headerFields: ["Content-Type": "application/json"]
                  )
            else {
                client?.urlProtocol(self, didFailWithError: URLError(.badURL))
                return
            }
            client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
            client?.urlProtocol(self, didLoad: Data(#"{"polished_text":"The thing is broken."}"#.utf8))
            client?.urlProtocolDidFinishLoading(self)
        }

        override func stopLoading() {}
    }

    /// Instructions are passed explicitly, sorted, rather than read from
    /// `settings.activePolishInstructions`: that property filters a `Dictionary`, whose
    /// iteration order is seeded per process. The request body is a `{label: true}` map
    /// so the order never reaches the wire, but the fixture generator must not depend
    /// on it either.
    static func run(session: Session, settings: AppSettings, text: String, instructions: [String]) throws -> Captured {
        URLProtocol.registerClass(Interceptor.self)
        defer { URLProtocol.unregisterClass(Interceptor.self) }
        Interceptor.captured = nil

        let service = PolishService(session: session, settings: settings)
        let done = DispatchSemaphore(value: 0)
        var failure: TranscriptionError?
        service.polish(text: text, instructions: instructions) { result in
            if case .failure(let error) = result { failure = error }
            done.signal()
        }

        guard done.wait(timeout: .now() + 30) == .success else {
            throw FixtureError("PolishService never called back; the interceptor did not fire")
        }
        if let failure {
            throw FixtureError("PolishService reported \(failure.userMessage)")
        }
        guard let captured = Interceptor.captured else {
            throw FixtureError("PolishService completed without a request reaching the interceptor")
        }
        return captured
    }
}
