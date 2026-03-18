import AppKit
import Foundation

enum AuthService {
    static func signInWithBrowser() {
        // Use wispr-flow:// scheme — this redirect URI is whitelisted in Supabase
        let redirectURI = "wispr-flow://auth/google/success"
        let encodedRedirect = redirectURI.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? redirectURI
        let authURL = "\(Constants.supabaseURL)/auth/v1/authorize?provider=google&redirect_to=\(encodedRedirect)"
        if let url = URL(string: authURL) {
            NSWorkspace.shared.open(url)
        }
    }

    static func handleCallback(url: URL, session: Session, completion: @escaping (Bool) -> Void) {
        // Supabase sends tokens as query params for custom URL schemes:
        // wispr-flow://auth/google/success?access_token=...&refresh_token=...&first_name=...&last_name=...
        let components = URLComponents(url: url, resolvingAgainstBaseURL: false)
        var params: [String: String] = [:]
        for item in components?.queryItems ?? [] {
            if let value = item.value { params[item.name] = value }
        }
        // Fallback: parse fragment (older Supabase behavior)
        if params["access_token"] == nil, let fragment = url.fragment {
            for pair in fragment.split(separator: "&") {
                let parts = pair.split(separator: "=", maxSplits: 1)
                if parts.count == 2 {
                    params[String(parts[0])] = String(parts[1]).removingPercentEncoding ?? String(parts[1])
                }
            }
        }

        guard let accessToken = params["access_token"],
              let refreshToken = params["refresh_token"] else {
            completion(false)
            return
        }

        session.accessToken = accessToken
        session.refreshToken = refreshToken
        session.expiresAt = Double(params["expires_at"] ?? "0") ?? 0

        // Extract everything from the JWT payload — no extra network call needed
        if let payload = decodeJWTPayload(accessToken) {
            session.userEmail = payload["email"] as? String
            if let meta = payload["user_metadata"] as? [String: Any] {
                session.avatarURL = meta["avatar_url"] as? String ?? meta["picture"] as? String
                let fullName = meta["full_name"] as? String ?? meta["name"] as? String ?? ""
                let parts = fullName.split(separator: " ", maxSplits: 1)
                session.userFirstName = parts.first.map(String.init) ?? params["first_name"]
                session.userLastName = parts.count > 1 ? String(parts[1]) : params["last_name"]
            }
        }
        // Fallback to callback URL params for name if JWT didn't have them
        if session.userFirstName == nil { session.userFirstName = params["first_name"] }
        if session.userLastName == nil { session.userLastName = params["last_name"] }

        session.save()
        completion(true)
    }

    /// Decode the payload of a JWT without verifying the signature.
    private static func decodeJWTPayload(_ token: String) -> [String: Any]? {
        let segments = token.split(separator: ".", omittingEmptySubsequences: false)
        guard segments.count >= 2 else { return nil }
        var base64 = String(segments[1])
            .replacingOccurrences(of: "-", with: "+")
            .replacingOccurrences(of: "_", with: "/")
        while base64.count % 4 != 0 { base64 += "=" }
        guard let data = Data(base64Encoded: base64) else { return nil }
        return try? JSONSerialization.jsonObject(with: data) as? [String: Any]
    }
}
