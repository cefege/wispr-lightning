//! Frontmost application identity and classification.
//!
//! The transcription backend keys personalization off the bundle id and picks
//! a formatting style from the coarse `AppKind`, so the classification table
//! is a wire contract, not a cosmetic detail. The exact sets are fixed by
//! `docs/parity/platform-spec.md` §5.

use objc2_app_kit::NSWorkspace;

use super::injector::ax;
use crate::{AppInfo, AppKind, ForegroundApp};

/// Chat clients: short, informal, no salutation.
const MESSAGING: &[&str] = &[
    "com.slack.Slack",
    "com.tinyspeck.slackmacgap",
    "net.whatsapp.WhatsApp",
    "com.tdesktop.Telegram",
    "org.whispersystems.signal-desktop",
    "com.discordapp.Discord",
];

/// Mail clients: full sentences, and the place the email signature is appended.
const EMAIL: &[&str] = &[
    "com.apple.mail",
    "com.microsoft.Outlook",
    "com.google.Gmail",
];

/// Assistant and code surfaces: prompts and identifiers survive verbatim.
const AI: &[&str] = &[
    "com.openai.chat",
    "com.anthropic.claudefordesktop",
    // Cursor ships under a ToDesktop-generated identifier.
    "com.todesktop.230313mzl4w4u92",
    "com.microsoft.VSCode",
];

/// Apps whose focused window exposes the current page through AX `AXURL`.
///
/// Checked before spending an accessibility round-trip: every other app would
/// return nothing, and AX calls are synchronous IPC into the target process.
const BROWSERS: &[&str] = &[
    "com.apple.Safari",
    "com.apple.SafariTechnologyPreview",
    "com.google.Chrome",
    "com.google.Chrome.beta",
    "com.google.Chrome.canary",
    "com.brave.Browser",
    "com.microsoft.edgemac",
    "com.vivaldi.Vivaldi",
    "com.operasoftware.Opera",
    "company.thebrowser.Browser",
    "org.mozilla.firefox",
];

/// Map a bundle identifier onto the wire-visible app category.
pub fn classify(bundle_id: &str) -> AppKind {
    if MESSAGING.contains(&bundle_id) {
        AppKind::Messaging
    } else if EMAIL.contains(&bundle_id) {
        AppKind::Email
    } else if AI.contains(&bundle_id) {
        AppKind::Ai
    } else {
        AppKind::Other
    }
}

pub struct MacForegroundApp;

impl ForegroundApp for MacForegroundApp {
    fn current(&self) -> AppInfo {
        let workspace = NSWorkspace::sharedWorkspace();
        let Some(app) = workspace.frontmostApplication() else {
            // No frontmost app happens during login-window and fast-user-switch
            // transitions. An all-empty record is the documented shape.
            return AppInfo::default();
        };

        let name = app
            .localizedName()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let bundle_id = app
            .bundleIdentifier()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let url = if BROWSERS.contains(&bundle_id.as_str()) {
            browser_url(app.processIdentifier()).unwrap_or_default()
        } else {
            String::new()
        };

        AppInfo {
            kind: classify(&bundle_id),
            name,
            bundle_id,
            url,
        }
    }
}

/// Read the address of the frontmost browser tab.
///
/// Chromium publishes it straight on the focused window as `AXURL`. WebKit
/// does not — Safari's window exposes `AXDocument` but leaves it unset for
/// anything that is not a local file — so the web area a few levels down has
/// to be found. The search stops at the first element carrying a URL, which is
/// that web area, so the page's own accessibility tree is never walked.
fn browser_url(pid: i32) -> Option<String> {
    let app = ax::application(pid);
    let window = ax::copy_element(&app, ax::FOCUSED_WINDOW)?;
    ax::copy_url(&window, ax::URL).or_else(|| find_url(&window))
}

/// Nodes and depth the web-area search may spend.
///
/// Both are hard stops. Every step is synchronous IPC into the browser, and
/// this runs at recording start, so an unbounded walk of a chrome window with
/// fifty tabs would be a visible stall.
const URL_SEARCH_NODES: usize = 192;
const URL_SEARCH_DEPTH: u32 = 10;

fn find_url(root: &objc2_application_services::AXUIElement) -> Option<String> {
    // The root itself was already checked by the caller.
    let mut stack: Vec<_> = ax::children(root).into_iter().map(|c| (c, 1u32)).collect();
    let mut visited = 0usize;
    while let Some((element, depth)) = stack.pop() {
        visited += 1;
        if visited > URL_SEARCH_NODES {
            break;
        }
        if let Some(url) = ax::copy_url(&element, ax::URL) {
            return Some(url);
        }
        if depth < URL_SEARCH_DEPTH {
            stack.extend(ax::children(&element).into_iter().map(|c| (c, depth + 1)));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_messaging_bundle_id_classifies_as_messaging() {
        for id in MESSAGING {
            assert_eq!(classify(id), AppKind::Messaging, "{id}");
        }
    }

    #[test]
    fn every_email_bundle_id_classifies_as_email() {
        for id in EMAIL {
            assert_eq!(classify(id), AppKind::Email, "{id}");
        }
    }

    #[test]
    fn every_ai_bundle_id_classifies_as_ai() {
        for id in AI {
            assert_eq!(classify(id), AppKind::Ai, "{id}");
        }
    }

    #[test]
    fn an_unlisted_bundle_id_classifies_as_other() {
        assert_eq!(classify("com.apple.Terminal"), AppKind::Other);
        assert_eq!(classify(""), AppKind::Other);
    }

    /// The email category drives the signature suffix, so an id landing in two
    /// tables would make the behaviour depend on check order.
    #[test]
    fn the_classification_tables_do_not_overlap() {
        let mut all: Vec<&str> = MESSAGING.iter().chain(EMAIL).chain(AI).copied().collect();
        let total = all.len();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), total, "a bundle id appears in two categories");
    }

    /// These strings go on the wire; the backend matches them exactly.
    #[test]
    fn the_wire_names_are_the_lowercase_category_labels() {
        assert_eq!(classify("com.slack.Slack").as_str(), "messaging");
        assert_eq!(classify("com.apple.mail").as_str(), "email");
        assert_eq!(classify("com.openai.chat").as_str(), "ai");
        assert_eq!(classify("nope").as_str(), "other");
    }
}
