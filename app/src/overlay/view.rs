//! What the pill renders for each [`OverlayState`]: the `data-state` key every
//! CSS rule is written against, and the label text (ui-spec §4.4, OVL-020..026).
//! Widths are not here: [`wl_shell::overlay_geometry::width_for`] is the one
//! table, shared by the window and the pill.

use wl_shell::ui::OverlayState;

pub fn key(state: &OverlayState) -> &'static str {
    match state {
        OverlayState::Hidden => "hidden",
        OverlayState::Recording => "recording",
        OverlayState::Locked => "locked",
        OverlayState::Processing => "processing",
        OverlayState::Inserting => "inserting",
        OverlayState::Retrying { .. } => "retrying",
        OverlayState::Error { .. } => "error",
        OverlayState::Recoverable { .. } => "recoverable",
    }
}

/// Main label. Empty while recording: the VU band is the entire indicator.
pub fn label(state: &OverlayState) -> String {
    match state {
        OverlayState::Hidden | OverlayState::Recording | OverlayState::Locked => String::new(),
        OverlayState::Processing => "Processing".to_string(),
        // U+2026, not three periods.
        OverlayState::Inserting => "Inserting\u{2026}".to_string(),
        OverlayState::Retrying { attempt, of } => format!("Retrying\u{2026} ({attempt}/{of})"),
        OverlayState::Error { message } | OverlayState::Recoverable { message } => message.clone(),
    }
}

/// Live recording: shows the VU strip, offers the cancel ✕, accepts levels,
/// and is the only mode the elapsed readout accompanies.
pub fn is_recording(state: &OverlayState) -> bool {
    matches!(state, OverlayState::Recording | OverlayState::Locked)
}

/// Entering these states resets the time-warning tint (OVL-034).
pub fn resets_warning(state: &OverlayState) -> bool {
    is_recording(state) || matches!(state, OverlayState::Processing | OverlayState::Inserting)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_match_the_spec_strings() {
        assert_eq!(label(&OverlayState::Recording), "");
        assert_eq!(label(&OverlayState::Inserting), "Inserting\u{2026}");
        assert_eq!(
            label(&OverlayState::Retrying { attempt: 2, of: 3 }),
            "Retrying\u{2026} (2/3)"
        );
        assert_eq!(
            label(&OverlayState::Recoverable {
                message: "No connection".into()
            }),
            "No connection"
        );
    }

    #[test]
    fn warning_resets_only_on_fresh_presentations() {
        assert!(resets_warning(&OverlayState::Locked));
        assert!(resets_warning(&OverlayState::Inserting));
        assert!(!resets_warning(&OverlayState::Retrying {
            attempt: 1,
            of: 3
        }));
        assert!(!resets_warning(&OverlayState::Error {
            message: String::new()
        }));
    }
}
