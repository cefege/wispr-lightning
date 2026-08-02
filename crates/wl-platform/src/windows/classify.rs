//! Pure helpers behind [`super::appinfo`]: classifying an executable and
//! making sense of whatever text a browser's address bar hands back.
//!
//! Windows has no bundle identifier, so the transcription backend receives the
//! executable basename and this table stands in for the macOS bundle-ID lists
//! in `AppInfoDetector`. Keeping the file free of Win32 is deliberate: it is
//! the part with real decisions in it, and it stays unit-testable on any host.

use crate::AppKind;

/// Executable basename (lowercased, `.exe` included) to app kind.
///
/// One row per macOS bundle ID in the Swift classifier, mapped to the Windows
/// build of the same product. Matching is exact rather than by prefix: `code.exe`
/// must not also claim `codeblocks.exe`, and channel variants that ship under
/// a different basename get their own row.
const TABLE: &[(&str, AppKind)] = &[
    // Messaging — com.slack.Slack, net.whatsapp.WhatsApp, com.tdesktop.Telegram,
    // org.whispersystems.signal-desktop, com.discordapp.Discord.
    ("slack.exe", AppKind::Messaging),
    ("whatsapp.exe", AppKind::Messaging),
    ("telegram.exe", AppKind::Messaging),
    ("signal.exe", AppKind::Messaging),
    ("discord.exe", AppKind::Messaging),
    ("discordptb.exe", AppKind::Messaging),
    ("discordcanary.exe", AppKind::Messaging),
    // Email — com.microsoft.Outlook, com.apple.mail.
    // `olk.exe` is the new (WebView2) Outlook, `outlook.exe` the classic one,
    // `hxoutlook.exe` the in-box Windows Mail app.
    ("outlook.exe", AppKind::Email),
    ("olk.exe", AppKind::Email),
    ("hxoutlook.exe", AppKind::Email),
    ("thunderbird.exe", AppKind::Email),
    ("mailspring.exe", AppKind::Email),
    // AI / editors — com.openai.chat, com.anthropic.claudefordesktop, Cursor,
    // com.microsoft.VSCode.
    ("chatgpt.exe", AppKind::Ai),
    ("claude.exe", AppKind::Ai),
    ("cursor.exe", AppKind::Ai),
    ("code.exe", AppKind::Ai),
    ("code - insiders.exe", AppKind::Ai),
    ("windsurf.exe", AppKind::Ai),
];

/// Browsers whose address bar is worth interrogating for a tab URL.
const BROWSERS: &[&str] = &[
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "brave.exe",
    "opera.exe",
    "opera_gx.exe",
    "vivaldi.exe",
    "chromium.exe",
    "arc.exe",
    "librewolf.exe",
    "waterfox.exe",
    "zen.exe",
];

/// URI schemes we accept verbatim from an address bar.
const SCHEMES: &[&str] = &[
    "http://",
    "https://",
    "file://",
    "ftp://",
    "about:",
    "chrome://",
    "edge://",
    "moz-extension://",
    "view-source:",
];

/// Reduce a path or basename to the lowercase basename used as `bundle_id`.
///
/// Accepts a full path so callers never have to remember which form they hold;
/// Windows accepts both separators in most APIs and so do we.
pub(crate) fn exe_basename(path: &str) -> String {
    path.rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase()
}

/// Classify an executable for the transcription backend's formatting hint.
pub(crate) fn classify(exe: &str) -> AppKind {
    let name = exe_basename(exe);
    TABLE
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, kind)| *kind)
        .unwrap_or(AppKind::Other)
}

/// Whether this executable is a browser, and therefore worth a UI Automation
/// round trip to read the focused tab's URL.
pub(crate) fn is_browser(exe: &str) -> bool {
    let name = exe_basename(exe);
    BROWSERS.contains(&name.as_str())
}

/// Turn raw address-bar text into a URL, or reject it.
///
/// Chromium elides `https://` and shows the user's in-progress typing while
/// the omnibox has focus, so the text is not necessarily a URL at all. We keep
/// what is there and never invent a scheme: a fabricated `https://` in front
/// of `localhost:3000` would be actively wrong, and the field is free-form
/// context for the backend rather than something we dereference.
pub(crate) fn normalize_browser_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
        return None;
    }
    let lowered = trimmed.to_ascii_lowercase();
    if SCHEMES.iter().any(|scheme| lowered.starts_with(scheme)) {
        return Some(trimmed.to_owned());
    }
    // Scheme-less: accept only something that could be a host, so a one-word
    // search term does not get reported as the current page.
    let host = trimmed.split(['/', '?', '#']).next().unwrap_or(trimmed);
    let host = host.split(':').next().unwrap_or(host);
    let looks_like_host = host == "localhost"
        || (host.contains('.')
            && !host.starts_with('.')
            && !host.ends_with('.')
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')));
    looks_like_host.then(|| trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_macos_bundle_id_has_a_windows_executable_counterpart() {
        assert_eq!(classify("slack.exe"), AppKind::Messaging);
        assert_eq!(classify("Discord.exe"), AppKind::Messaging);
        assert_eq!(classify("OUTLOOK.EXE"), AppKind::Email);
        assert_eq!(classify("olk.exe"), AppKind::Email);
        assert_eq!(classify("Code.exe"), AppKind::Ai);
        assert_eq!(classify("Cursor.exe"), AppKind::Ai);
    }

    #[test]
    fn an_unknown_executable_is_other_rather_than_a_guess() {
        assert_eq!(classify("notepad.exe"), AppKind::Other);
        assert_eq!(classify(""), AppKind::Other);
    }

    #[test]
    fn classification_uses_the_basename_of_a_full_path() {
        assert_eq!(
            classify(r"C:\Users\mike\AppData\Local\slack\app-4.0\slack.exe"),
            AppKind::Messaging
        );
        assert_eq!(
            classify("C:/Program Files/Mozilla/thunderbird.exe"),
            AppKind::Email
        );
    }

    #[test]
    fn a_longer_name_sharing_a_prefix_is_not_matched() {
        // `code.exe` must not also claim these.
        assert_eq!(classify("codeblocks.exe"), AppKind::Other);
        assert_eq!(classify("vscode.exe"), AppKind::Other);
        assert_eq!(classify("slackbuild.exe"), AppKind::Other);
    }

    #[test]
    fn browsers_are_recognised_regardless_of_path_or_case() {
        assert!(is_browser(
            r"C:\Program Files\Google\Chrome\Application\chrome.exe"
        ));
        assert!(is_browser("MSEDGE.EXE"));
        assert!(!is_browser("slack.exe"));
    }

    #[test]
    fn an_address_bar_showing_a_url_is_reported_verbatim() {
        assert_eq!(
            normalize_browser_url("https://example.com/a?b=1"),
            Some("https://example.com/a?b=1".into())
        );
        assert_eq!(
            normalize_browser_url("  about:blank  "),
            Some("about:blank".into())
        );
    }

    #[test]
    fn a_scheme_less_host_is_kept_without_inventing_a_scheme() {
        assert_eq!(
            normalize_browser_url("example.com/path"),
            Some("example.com/path".into())
        );
        assert_eq!(
            normalize_browser_url("localhost:3000/x"),
            Some("localhost:3000/x".into())
        );
    }

    #[test]
    fn a_half_typed_search_query_is_not_reported_as_a_url() {
        assert_eq!(normalize_browser_url("how to cook rice"), None);
        assert_eq!(normalize_browser_url("weather"), None);
        assert_eq!(normalize_browser_url(""), None);
        assert_eq!(normalize_browser_url("   "), None);
    }
}
