import Foundation

class Session {
    var accessToken: String?
    var refreshToken: String?
    var userId: String?
    var userEmail: String?
    var userFirstName: String?
    var userLastName: String?
    var avatarURL: String?
    var expiresAt: TimeInterval = 0
    let sessionId: String = UUID().uuidString

    var isValid: Bool {
        guard accessToken != nil else { return false }
        if expiresAt > 0 && Date().timeIntervalSince1970 > expiresAt - 60 {
            return false
        }
        return true
    }

    static let wisprFlowSessionURL: URL = {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/Wispr Flow/session.json")
    }()

    static let liteSessionURL: URL = {
        let dir = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/WisprLightning")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.appendingPathComponent("session.json")
    }()

    func load() -> Bool {
        if loadFromURL(Self.liteSessionURL) { return true }
        // One-time migration: if no Lightning session, try Wispr Flow and copy it over
        if loadFromURL(Self.wisprFlowSessionURL) {
            save()  // Write to Lightning's own file so future launches don't need Wispr Flow
            NSLog("Wispr Lightning: Migrated session from Wispr Flow (%@)", userEmail ?? "unknown")
            return true
        }
        return false
    }

    private func loadFromURL(_ url: URL) -> Bool {
        guard FileManager.default.fileExists(atPath: url.path),
              let data = try? Data(contentsOf: url),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return false
        }
        return parseSession(json, source: url.path)
    }

    private func parseSession(_ data: [String: Any], source: String) -> Bool {
        // Find the auth-token key
        guard let tokenKey = data.keys.first(where: { $0.contains("auth-token") }) else {
            NSLog("Wispr Lightning: No auth-token key found in %@", source)
            return false
        }

        let session: [String: Any]
        if let sessionStr = data[tokenKey] as? String,
           let parsed = try? JSONSerialization.jsonObject(with: Data(sessionStr.utf8)) as? [String: Any] {
            session = parsed
        } else if let dict = data[tokenKey] as? [String: Any] {
            session = dict
        } else {
            return false
        }

        accessToken = session["access_token"] as? String
        refreshToken = session["refresh_token"] as? String
        expiresAt = session["expires_at"] as? TimeInterval ?? 0

        if let user = session["user"] as? [String: Any] {
            userId = user["id"] as? String
            userEmail = user["email"] as? String

            if let meta = user["user_metadata"] as? [String: Any] {
                avatarURL = meta["avatar_url"] as? String ?? meta["picture"] as? String
                let fullName = meta["full_name"] as? String ?? meta["name"] as? String ?? ""
                let parts = fullName.split(separator: " ", maxSplits: 1)
                userFirstName = parts.first.map(String.init) ?? ""
                userLastName = parts.count > 1 ? String(parts[1]) : ""
            }
        }

        guard accessToken != nil else { return false }

        // If avatar or name missing from saved file, enrich from JWT payload
        if avatarURL == nil || userEmail == nil, let token = accessToken {
            enrichFromJWT(token)
        }

        NSLog("Wispr Lightning: Session loaded from %@ (user: %@)", source, userEmail ?? "unknown")
        return true
    }

    private func enrichFromJWT(_ token: String) {
        let segments = token.split(separator: ".", omittingEmptySubsequences: false)
        guard segments.count >= 2 else { return }
        var base64 = String(segments[1])
            .replacingOccurrences(of: "-", with: "+")
            .replacingOccurrences(of: "_", with: "/")
        while base64.count % 4 != 0 { base64 += "=" }
        guard let data = Data(base64Encoded: base64),
              let payload = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return }

        if userEmail == nil { userEmail = payload["email"] as? String }
        // Use JWT exp as the authoritative expiry — callback URLs may omit expires_at
        if let exp = payload["exp"] as? TimeInterval, exp > Date().timeIntervalSince1970 {
            expiresAt = exp
        }
        if let meta = payload["user_metadata"] as? [String: Any] {
            if avatarURL == nil { avatarURL = meta["avatar_url"] as? String ?? meta["picture"] as? String }
            if (userFirstName == nil || userFirstName!.isEmpty),
               let fullName = meta["full_name"] as? String ?? meta["name"] as? String {
                let parts = fullName.split(separator: " ", maxSplits: 1)
                userFirstName = parts.first.map(String.init) ?? ""
                userLastName = parts.count > 1 ? String(parts[1]) : ""
            }
        }
    }

    func refresh(completion: @escaping (Bool) -> Void) {
        guard let refreshToken = refreshToken else {
            completion(false)
            return
        }

        let url = URL(string: "\(Constants.supabaseURL)/auth/v1/token?grant_type=refresh_token")!
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue(Constants.supabaseAnonKey, forHTTPHeaderField: "apikey")
        request.setValue("Bearer \(Constants.supabaseAnonKey)", forHTTPHeaderField: "Authorization")
        request.httpBody = try? JSONSerialization.data(withJSONObject: ["refresh_token": refreshToken])

        URLSession.shared.dataTask(with: request) { [weak self] data, response, error in
            guard let self = self,
                  let data = data,
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let newAccessToken = json["access_token"] as? String,
                  let newRefreshToken = json["refresh_token"] as? String else {
                NSLog("Wispr Lightning: Token refresh failed: %@", error?.localizedDescription ?? "unknown")
                completion(false)
                return
            }
            self.accessToken = newAccessToken
            self.refreshToken = newRefreshToken
            let absExpiry = json["expires_at"] as? TimeInterval ?? 0
            let relExpiry = json["expires_in"] as? TimeInterval ?? 0
            self.expiresAt = absExpiry > Date().timeIntervalSince1970
                ? absExpiry
                : (relExpiry > 0 ? Date().timeIntervalSince1970 + relExpiry : 0)
            self.enrichFromJWT(newAccessToken)  // also sets expiresAt from JWT exp if still 0
            self.save()
            wLogVerbose("Token refresh response: \(String(data: data, encoding: .utf8)?.prefix(300) ?? "")")
            NSLog("Wispr Lightning: Token refreshed successfully")
            completion(true)
        }.resume()
    }

    func save() {
        var userMeta: [String: Any] = [
            "full_name": "\(userFirstName ?? "") \(userLastName ?? "")"
        ]
        if let avatar = avatarURL { userMeta["avatar_url"] = avatar }

        let sessionData: [String: Any] = [
            "sb-dodjkfqhwrzqjwkfnthl-auth-token": [
                "access_token": accessToken ?? "",
                "refresh_token": refreshToken ?? "",
                "expires_at": expiresAt,
                "user": [
                    "id": userId ?? "",
                    "email": userEmail ?? "",
                    "user_metadata": userMeta
                ]
            ] as [String: Any]
        ]
        // Serialize the inner dict as a JSON string to match the original format
        if let innerData = try? JSONSerialization.data(withJSONObject: sessionData["sb-dodjkfqhwrzqjwkfnthl-auth-token"]!),
           let innerStr = String(data: innerData, encoding: .utf8) {
            let outer: [String: Any] = ["sb-dodjkfqhwrzqjwkfnthl-auth-token": innerStr]
            if let data = try? JSONSerialization.data(withJSONObject: outer, options: .prettyPrinted) {
                try? data.write(to: Self.liteSessionURL)
            }
        }
    }

    func clear() {
        accessToken = nil
        refreshToken = nil
        userId = nil
        userEmail = nil
        userFirstName = nil
        userLastName = nil
        expiresAt = 0
        try? FileManager.default.removeItem(at: Self.liteSessionURL)
        NotificationCenter.default.post(name: .sessionChanged, object: nil)
    }
}
