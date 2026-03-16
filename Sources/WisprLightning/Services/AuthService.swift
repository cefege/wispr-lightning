import AppKit

enum AuthService {
    static func signInWithBrowser() {
        // Open Supabase Google OAuth URL with redirect to our URL scheme
        let redirectURI = "wisprlightning://auth/callback"
        let authURL = "\(Constants.supabaseURL)/auth/v1/authorize?provider=google&redirect_to=\(redirectURI)"
        if let url = URL(string: authURL) {
            NSWorkspace.shared.open(url)
        }
    }

    static func handleCallback(url: URL, session: Session, completion: @escaping (Bool) -> Void) {
        // Parse tokens from URL fragment: wisprlightning://auth/callback#access_token=...&refresh_token=...
        guard let fragment = url.fragment ?? url.query else {
            completion(false)
            return
        }

        var params: [String: String] = [:]
        for pair in fragment.split(separator: "&") {
            let parts = pair.split(separator: "=", maxSplits: 1)
            if parts.count == 2 {
                params[String(parts[0])] = String(parts[1]).removingPercentEncoding ?? String(parts[1])
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

        // Fetch user profile
        fetchProfile(accessToken: accessToken) { email, firstName, lastName in
            session.userEmail = email
            session.userFirstName = firstName
            session.userLastName = lastName
            session.save()
            completion(true)
        }
    }

    private static func fetchProfile(accessToken: String, completion: @escaping (String?, String?, String?) -> Void) {
        guard let url = URL(string: "\(Constants.apiURL)/api/v1/user/profile") else {
            completion(nil, nil, nil)
            return
        }

        var request = URLRequest(url: url)
        request.setValue("Bearer \(accessToken)", forHTTPHeaderField: "Authorization")

        URLSession.shared.dataTask(with: request) { data, _, _ in
            guard let data = data,
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                completion(nil, nil, nil)
                return
            }
            let email = json["email"] as? String
            let firstName = json["first_name"] as? String
            let lastName = json["last_name"] as? String
            completion(email, firstName, lastName)
        }.resume()
    }
}
