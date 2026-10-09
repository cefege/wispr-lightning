//! Overlay geometry and click policy as pure functions: panel size per state,
//! placement in the work area, and which states accept clicks. No display
//! server or window handle is needed, so all of it is testable here.

use std::time::Duration;

use crate::ui::OverlayState;

/// The panel's height never changes (OVL-017).
pub const OVERLAY_HEIGHT: f64 = 36.0;

/// Distance from the bottom of the work area to the bottom of the panel, from
/// the Swift `y = visibleFrame.minY + 50` (ui-spec §4.3).
pub const BOTTOM_MARGIN: f64 = 50.0;

/// A transient error dismisses itself after exactly this long (OVL-024).
pub const ERROR_DISMISS: Duration = Duration::from_millis(3000);

/// Width the overlay is built at, so its first real show is a resize rather
/// than a creation (OVL-041).
pub const INITIAL_WIDTH: f64 = 120.0;

// ---------------------------------------------------------------------------
// Geometry — pure, and therefore testable without a display server
// ---------------------------------------------------------------------------

/// A rectangle in logical points, top-left origin (the Tauri convention).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The window width for a given overlay state, or `None` when the state puts
/// nothing on screen.
///
/// Widths come from ui-spec §4.4 / OVL-020..OVL-026 and OVL-032.
///
/// One deliberate collapse: the Swift `showRetryableError` was 260 wide without
/// a save handler and 300 with one, but [`OverlayState::Recoverable`] carries
/// no such flag — audio is always spooled before transcription, so Save is
/// always offered. 300 is therefore the only reachable width, and 260 is
/// unreachable rather than missing.
pub fn width_for(state: &OverlayState, elapsed_visible: bool) -> Option<f64> {
    Some(match state {
        OverlayState::Hidden => return None,
        // OVL-032: the panel jumps 120 -> 200 the first time the elapsed timer
        // appears, at 30 seconds.
        OverlayState::Recording | OverlayState::Locked if elapsed_visible => 200.0,
        // v2-ui-spec §1.2: 130, widened from 120 to fit the 88pt VU strip
        // inside the stack's 16pt edge insets.
        OverlayState::Recording | OverlayState::Locked => 130.0,
        OverlayState::Processing | OverlayState::Inserting => 145.0,
        OverlayState::Retrying { .. } => 175.0,
        OverlayState::Error { .. } => 180.0,
        OverlayState::Recoverable { .. } => 300.0,
    })
}

/// Bottom-centre of `work_area`, [`BOTTOM_MARGIN`] up from its bottom edge.
///
/// `work_area` excludes the menu bar and the Dock / taskbar, which is what both
/// `NSScreen.visibleFrame` and `SPI_GETWORKAREA` give.
pub fn overlay_frame(work_area: Rect, width: f64) -> Rect {
    Rect {
        x: work_area.x + (work_area.width - width) / 2.0,
        // AppKit measures y up from the bottom; Tauri measures it down from the
        // top, so the Swift `minY + 50` becomes "the work area's bottom edge,
        // less the margin, less our own height".
        y: work_area.y + work_area.height - BOTTOM_MARGIN - OVERLAY_HEIGHT,
        width,
        height: OVERLAY_HEIGHT,
    }
}

/// Whether the overlay accepts clicks in this state.
///
/// Only two states carry a control. [`OverlayState::Recoverable`] has the
/// Retry / Save / ✕ row (OVL-007); Recording and Locked have the
/// hover-revealed cancel ✕ (v2-ui-spec §1.7), which needs both the click and
/// the `mouseenter` that reveals it, and gets neither through a click-through
/// window. Everything else is passive, and a passive overlay that swallowed
/// clicks would make the strip of desktop it covers unusable.
///
/// Accepting clicks is not the same as taking focus. The window stays
/// `focusable(false)` and, on macOS, a genuine non-activating `NSPanel`: a
/// click on the ✕ must not move focus off the app being dictated into, or the
/// transcript lands in the wrong window.
pub fn accepts_clicks(state: &OverlayState) -> bool {
    matches!(
        state,
        OverlayState::Recoverable { .. } | OverlayState::Recording | OverlayState::Locked
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::Elapsed;

    /// A 1440x900 laptop display with a 25-point menu bar at the top.
    fn work_area() -> Rect {
        Rect {
            x: 0.0,
            y: 25.0,
            width: 1440.0,
            height: 875.0,
        }
    }

    fn widths(elapsed_visible: bool) -> Vec<(&'static str, Option<f64>)> {
        vec![
            ("Hidden", width_for(&OverlayState::Hidden, elapsed_visible)),
            (
                "Recording",
                width_for(&OverlayState::Recording, elapsed_visible),
            ),
            ("Locked", width_for(&OverlayState::Locked, elapsed_visible)),
            (
                "Processing",
                width_for(&OverlayState::Processing, elapsed_visible),
            ),
            (
                "Inserting",
                width_for(&OverlayState::Inserting, elapsed_visible),
            ),
            (
                "Retrying",
                width_for(
                    &OverlayState::Retrying { attempt: 1, of: 3 },
                    elapsed_visible,
                ),
            ),
            (
                "Error",
                width_for(
                    &OverlayState::Error {
                        message: "Timed out".into(),
                    },
                    elapsed_visible,
                ),
            ),
            (
                "Recoverable",
                width_for(
                    &OverlayState::Recoverable {
                        message: "Connection failed".into(),
                    },
                    elapsed_visible,
                ),
            ),
        ]
    }

    /// v2-ui-spec §1.9, every row.
    #[test]
    fn state_widths_match_the_spec() {
        assert_eq!(
            widths(false),
            vec![
                ("Hidden", None),
                // 130, not 120: the pill now carries the 88pt VU strip.
                ("Recording", Some(130.0)),
                ("Locked", Some(130.0)),
                ("Processing", Some(145.0)),
                ("Inserting", Some(145.0)),
                ("Retrying", Some(175.0)),
                ("Error", Some(180.0)),
                ("Recoverable", Some(300.0)),
            ]
        );
    }

    /// OVL-032: only the two recording states widen for the elapsed readout.
    #[test]
    fn the_elapsed_timer_widens_only_the_recording_states() {
        assert_eq!(
            widths(true),
            vec![
                ("Hidden", None),
                ("Recording", Some(200.0)),
                ("Locked", Some(200.0)),
                ("Processing", Some(145.0)),
                ("Inserting", Some(145.0)),
                ("Retrying", Some(175.0)),
                ("Error", Some(180.0)),
                ("Recoverable", Some(300.0)),
            ]
        );
    }

    #[test]
    fn the_height_is_thirty_six_in_every_state() {
        for (_, width) in widths(false).into_iter().chain(widths(true)) {
            let Some(width) = width else { continue };
            assert_eq!(overlay_frame(work_area(), width).height, OVERLAY_HEIGHT);
        }
    }

    /// OVL-017: horizontally centred, and 50 points up from the bottom of the
    /// work area.
    #[test]
    fn the_frame_is_bottom_centred_in_the_work_area() {
        let frame = overlay_frame(work_area(), 120.0);
        assert_eq!(frame.x, 660.0, "centred: (1440 - 120) / 2");
        // The work area's bottom edge is y = 25 + 875 = 900; the panel's bottom
        // sits 50 above that, and its top a further 36 above.
        assert_eq!(frame.y, 900.0 - 50.0 - 36.0);
        assert_eq!(frame.y + frame.height, 900.0 - BOTTOM_MARGIN);
    }

    #[test]
    fn every_state_stays_centred_on_the_same_axis() {
        let centre = work_area().x + work_area().width / 2.0;
        for (state, width) in widths(false) {
            let Some(width) = width else { continue };
            let frame = overlay_frame(work_area(), width);
            assert_eq!(frame.x + frame.width / 2.0, centre, "{state} is off centre");
        }
    }

    /// A monitor whose work area does not start at the origin — a second
    /// display to the right, or a Windows taskbar docked to the left.
    #[test]
    fn the_frame_respects_an_offset_work_area() {
        let offset = Rect {
            x: 1440.0,
            y: 0.0,
            width: 1920.0,
            height: 1040.0,
        };
        let frame = overlay_frame(offset, 300.0);
        assert_eq!(frame.x, 1440.0 + (1920.0 - 300.0) / 2.0);
        assert_eq!(frame.y, 1040.0 - 50.0 - 36.0);
    }

    /// Exactly the states with a control accept clicks; everything else is
    /// click-through so the overlay never blocks what is underneath it
    /// (OVL-007, and v2-ui-spec §1.7 for the recording ✕).
    #[test]
    fn only_the_states_with_a_control_accept_clicks() {
        // The hover-revealed cancel ✕ lives in these two, and a click-through
        // window would receive neither the click nor the hover that reveals it.
        assert!(accepts_clicks(&OverlayState::Recording));
        assert!(accepts_clicks(&OverlayState::Locked));
        // Retry / Save / ✕.
        assert!(accepts_clicks(&OverlayState::Recoverable {
            message: "Connection failed".into()
        }));

        assert!(!accepts_clicks(&OverlayState::Hidden));
        assert!(!accepts_clicks(&OverlayState::Processing));
        assert!(!accepts_clicks(&OverlayState::Inserting));
        assert!(!accepts_clicks(&OverlayState::Retrying {
            attempt: 2,
            of: 3
        }));
        assert!(!accepts_clicks(&OverlayState::Error {
            message: "Timed out".into()
        }));
    }

    /// The event payloads are a frozen part of the IPC contract.
    #[test]
    fn overlay_states_serialise_to_the_contract_shape() {
        let json = |state: &OverlayState| serde_json::to_string(state).expect("serialisable");
        assert_eq!(json(&OverlayState::Hidden), r#""Hidden""#);
        assert_eq!(json(&OverlayState::Recording), r#""Recording""#);
        assert_eq!(json(&OverlayState::Locked), r#""Locked""#);
        assert_eq!(json(&OverlayState::Processing), r#""Processing""#);
        assert_eq!(json(&OverlayState::Inserting), r#""Inserting""#);
        assert_eq!(
            json(&OverlayState::Retrying { attempt: 1, of: 3 }),
            r#"{"Retrying":{"attempt":1,"of":3}}"#
        );
        assert_eq!(
            json(&OverlayState::Error {
                message: "Timed out".into()
            }),
            r#"{"Error":{"message":"Timed out"}}"#
        );
        assert_eq!(
            json(&OverlayState::Recoverable {
                message: "Connection failed".into()
            }),
            r#"{"Recoverable":{"message":"Connection failed"}}"#
        );
    }

    #[test]
    fn the_elapsed_payload_keeps_its_field_names() {
        let json = serde_json::to_string(&Elapsed {
            label: Some("9:05 \u{26A0}\u{FE0F}".into()),
            warning: 1,
        })
        .expect("serialisable");
        assert_eq!(json, r#"{"label":"9:05 ⚠️","warning":1}"#);
        assert_eq!(
            serde_json::to_string(&Elapsed::default()).expect("serialisable"),
            r#"{"label":null,"warning":0}"#
        );
    }
}
