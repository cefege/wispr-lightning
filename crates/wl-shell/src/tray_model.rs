//! The tray menu as data: order, labels, enabled and checked flags, with no
//! toolkit types involved. The Tauri shell realises this description as real
//! menu items; the split keeps which item carries the check mark testable
//! without a display server.
//!
//! Covers MATRIX TRY-001 through TRY-021.

use wl_platform::audio::InputDevice;
/// Stable id of the tray icon, so a second `TrayIconBuilder` can never create a
/// duplicate.
pub const TRAY_ID: &str = "wispr-lightning";

pub const ID_LAST_TRANSCRIPTION: &str = "tray:last-transcription";
pub const ID_PAUSE: &str = "tray:pause";
pub const ID_NATURAL_MODE: &str = "tray:natural-mode";
pub const ID_SETTINGS: &str = "tray:settings";
pub const ID_QUIT: &str = "tray:quit";

/// Device items are `tray:device:<id>`; the system-default item has an empty
/// suffix, which is exactly the `None` it stands for.
pub const ID_DEVICE_PREFIX: &str = "tray:device:";

/// Characters of the last dictation shown in the menu, matching the Swift
/// `prefix(60)`.
const PREVIEW_CHARS: usize = 60;

/// `Cmd+,` on macOS and `Ctrl+,` on Windows, from one string (TRY-018).
const SETTINGS_ACCELERATOR: &str = "CmdOrCtrl+,";

// ---------------------------------------------------------------------------
// The pure menu model
// ---------------------------------------------------------------------------

/// One entry in the tray menu, independent of any windowing toolkit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuNode {
    Item {
        id: String,
        label: String,
        enabled: bool,
        accelerator: Option<&'static str>,
    },
    /// An item that carries a check mark. Always enabled: every checkable item
    /// in this menu is also a toggle.
    Check {
        id: String,
        label: String,
        checked: bool,
    },
    Separator,
    Submenu {
        label: String,
        children: Vec<MenuNode>,
    },
}

/// Everything the menu's shape depends on.
#[derive(Debug, Clone, Copy)]
pub struct MenuInput<'a> {
    /// The most recent dictation, or `None` if there has not been one this
    /// session.
    pub last_transcription: Option<&'a str>,
    pub devices: &'a [InputDevice],
    /// `Settings::mic_device_id`.
    pub mic_device_id: Option<&'a str>,
    pub hotkey_paused: bool,
    pub natural_mode: bool,
}

/// The device the check mark belongs on, or `None` for "System Default".
///
/// TRY-021: a `settings.json` carried from macOS to Windows holds a
/// `coreaudio:<uid>` that matches no WASAPI endpoint. Falling back to the
/// system default here — rather than leaving every item unchecked — makes the
/// menu agree with what the audio backend will actually open.
pub fn resolve_device<'a>(devices: &'a [InputDevice], stored: Option<&str>) -> Option<&'a str> {
    let stored = stored?;
    devices
        .iter()
        .find(|d| d.id == stored)
        .map(|d| d.id.as_str())
}

/// One line of menu text for a transcript.
///
/// Interior newlines are collapsed to single spaces: a raw `\n` renders as a
/// second line in an `NSMenuItem` and as a box glyph in a Win32 menu, so the
/// verbatim text is unusable as a label. The full text is still what gets
/// copied — only the label is shortened.
pub fn preview(text: &str) -> String {
    let flattened = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flattened.chars().count() > PREVIEW_CHARS {
        let head: String = flattened.chars().take(PREVIEW_CHARS).collect();
        format!("{head}\u{2026}")
    } else {
        flattened
    }
}

/// Build the menu description. TRY-010 fixes the order; every other TRY row in
/// section 12 fixes one of the labels or flags.
pub fn menu_model(input: &MenuInput<'_>) -> Vec<MenuNode> {
    let mut nodes = Vec::with_capacity(8);

    // TRY-011 / TRY-012.
    nodes.push(match input.last_transcription.filter(|t| !t.is_empty()) {
        Some(text) => MenuNode::Item {
            id: ID_LAST_TRANSCRIPTION.to_string(),
            label: preview(text),
            enabled: true,
            accelerator: None,
        },
        None => MenuNode::Item {
            id: ID_LAST_TRANSCRIPTION.to_string(),
            label: "No recent dictation".to_string(),
            enabled: false,
            accelerator: None,
        },
    });

    // TRY-013.
    nodes.push(MenuNode::Separator);

    // TRY-014 / TRY-021.
    let selected = resolve_device(input.devices, input.mic_device_id);
    let mut devices = Vec::with_capacity(input.devices.len() + 2);
    devices.push(MenuNode::Check {
        id: ID_DEVICE_PREFIX.to_string(),
        label: "System Default".to_string(),
        checked: selected.is_none(),
    });
    if !input.devices.is_empty() {
        devices.push(MenuNode::Separator);
        for device in input.devices {
            devices.push(MenuNode::Check {
                id: format!("{ID_DEVICE_PREFIX}{}", device.id),
                label: device.name.clone(),
                checked: selected == Some(device.id.as_str()),
            });
        }
    }
    nodes.push(MenuNode::Submenu {
        label: "Input Device".to_string(),
        children: devices,
    });

    // TRY-016: the title flips and the item is checked while paused.
    nodes.push(MenuNode::Check {
        id: ID_PAUSE.to_string(),
        label: if input.hotkey_paused {
            "Resume hotkey".to_string()
        } else {
            "Pause hotkey".to_string()
        },
        checked: input.hotkey_paused,
    });

    // TRY-017.
    nodes.push(MenuNode::Check {
        id: ID_NATURAL_MODE.to_string(),
        label: "Natural Mode".to_string(),
        checked: input.natural_mode,
    });

    // TRY-018.
    nodes.push(MenuNode::Item {
        id: ID_SETTINGS.to_string(),
        label: "Settings".to_string(),
        enabled: true,
        accelerator: Some(SETTINGS_ACCELERATOR),
    });

    // TRY-019 / TRY-020.
    nodes.push(MenuNode::Separator);
    nodes.push(MenuNode::Item {
        id: ID_QUIT.to_string(),
        label: "Quit Wispr Lightning".to_string(),
        enabled: true,
        accelerator: None,
    });

    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str, name: &str) -> InputDevice {
        InputDevice {
            id: id.to_string(),
            name: name.to_string(),
            is_default: false,
        }
    }

    fn devices() -> Vec<InputDevice> {
        vec![
            device("coreaudio:built-in", "MacBook Pro Microphone"),
            device("coreaudio:yeti", "Blue Yeti"),
        ]
    }

    fn base(devices: &[InputDevice]) -> MenuInput<'_> {
        MenuInput {
            last_transcription: None,
            devices,
            mic_device_id: None,
            hotkey_paused: false,
            natural_mode: false,
        }
    }

    fn label(node: &MenuNode) -> &str {
        match node {
            MenuNode::Item { label, .. }
            | MenuNode::Check { label, .. }
            | MenuNode::Submenu { label, .. } => label,
            MenuNode::Separator => "-",
        }
    }

    fn labels(nodes: &[MenuNode]) -> Vec<&str> {
        nodes.iter().map(label).collect()
    }

    fn checked(nodes: &[MenuNode]) -> Vec<&str> {
        nodes
            .iter()
            .filter_map(|n| match n {
                MenuNode::Check {
                    label,
                    checked: true,
                    ..
                } => Some(label.as_str()),
                _ => None,
            })
            .collect()
    }

    fn submenu(nodes: &[MenuNode]) -> &[MenuNode] {
        match &nodes[2] {
            MenuNode::Submenu { children, .. } => children,
            other => panic!("expected the Input Device submenu at index 2, found {other:?}"),
        }
    }

    #[test]
    fn top_level_order_matches_try_010() {
        let devices = devices();
        assert_eq!(
            labels(&menu_model(&base(&devices))),
            vec![
                "No recent dictation",
                "-",
                "Input Device",
                "Pause hotkey",
                "Natural Mode",
                "Settings",
                "-",
                "Quit Wispr Lightning",
            ]
        );
    }

    #[test]
    fn with_no_dictation_the_first_item_is_disabled() {
        let nodes = menu_model(&base(&[]));
        assert!(matches!(
            &nodes[0],
            MenuNode::Item { label, enabled: false, .. } if label == "No recent dictation"
        ));
    }

    #[test]
    fn a_dictation_becomes_an_enabled_preview_item() {
        let devices = devices();
        let nodes = menu_model(&MenuInput {
            last_transcription: Some("hello there"),
            ..base(&devices)
        });
        assert!(matches!(
            &nodes[0],
            MenuNode::Item { id, label, enabled: true, .. }
                if id == ID_LAST_TRANSCRIPTION && label == "hello there"
        ));
    }

    #[test]
    fn an_empty_dictation_is_treated_as_no_dictation() {
        let nodes = menu_model(&MenuInput {
            last_transcription: Some(""),
            ..base(&[])
        });
        assert_eq!(label(&nodes[0]), "No recent dictation");
    }

    #[test]
    fn the_preview_elides_at_sixty_characters() {
        assert_eq!(
            preview(&"a".repeat(61)),
            format!("{}\u{2026}", "a".repeat(60))
        );
        let exact = "b".repeat(60);
        assert_eq!(preview(&exact), exact);
    }

    #[test]
    fn the_preview_collapses_newlines_into_one_line() {
        assert_eq!(
            preview("first line\n\nsecond   line"),
            "first line second line"
        );
    }

    #[test]
    fn the_device_submenu_lists_default_then_a_separator_then_every_device() {
        let devices = devices();
        let nodes = menu_model(&base(&devices));
        assert_eq!(label(&nodes[2]), "Input Device");
        assert_eq!(
            labels(submenu(&nodes)),
            vec!["System Default", "-", "MacBook Pro Microphone", "Blue Yeti"]
        );
    }

    #[test]
    fn an_empty_device_list_leaves_no_dangling_separator() {
        let nodes = menu_model(&base(&[]));
        assert_eq!(labels(submenu(&nodes)), vec!["System Default"]);
    }

    #[test]
    fn system_default_is_checked_when_no_device_is_stored() {
        let devices = devices();
        let nodes = menu_model(&base(&devices));
        assert_eq!(checked(submenu(&nodes)), vec!["System Default"]);
    }

    #[test]
    fn the_stored_device_carries_the_only_check_mark() {
        let devices = devices();
        let nodes = menu_model(&MenuInput {
            mic_device_id: Some("coreaudio:yeti"),
            ..base(&devices)
        });
        assert_eq!(checked(submenu(&nodes)), vec!["Blue Yeti"]);
    }

    /// TRY-021: a macOS `settings.json` opened on Windows names a device that
    /// does not exist there. The check mark must land on System Default, never
    /// nowhere.
    #[test]
    fn an_unresolvable_device_id_falls_back_to_system_default() {
        let stored = "coreaudio:AppleUSBAudioEngine:Blue:Yeti";
        let windows_devices = vec![
            device("wasapi:{0.0.1.0}", "Microphone (Realtek)"),
            device("wasapi:{0.0.1.1}", "Headset"),
        ];
        let nodes = menu_model(&MenuInput {
            mic_device_id: Some(stored),
            ..base(&windows_devices)
        });
        assert_eq!(checked(submenu(&nodes)), vec!["System Default"]);
        assert_eq!(resolve_device(&windows_devices, Some(stored)), None);
    }

    #[test]
    fn the_device_item_id_round_trips_the_device_id() {
        let devices = devices();
        let nodes = menu_model(&base(&devices));
        let ids: Vec<Option<&str>> = submenu(&nodes)
            .iter()
            .filter_map(|n| match n {
                MenuNode::Check { id, .. } => Some(id.strip_prefix(ID_DEVICE_PREFIX)),
                _ => None,
            })
            .collect();
        assert_eq!(
            ids,
            vec![Some(""), Some("coreaudio:built-in"), Some("coreaudio:yeti")]
        );
    }

    #[test]
    fn the_pause_item_flips_its_title_and_check_together() {
        assert_eq!(
            menu_model(&base(&[]))[3],
            MenuNode::Check {
                id: ID_PAUSE.to_string(),
                label: "Pause hotkey".to_string(),
                checked: false,
            }
        );
        assert_eq!(
            menu_model(&MenuInput {
                hotkey_paused: true,
                ..base(&[])
            })[3],
            MenuNode::Check {
                id: ID_PAUSE.to_string(),
                label: "Resume hotkey".to_string(),
                checked: true,
            }
        );
    }

    #[test]
    fn natural_mode_reflects_the_settings_field() {
        assert_eq!(
            menu_model(&base(&[]))[4],
            MenuNode::Check {
                id: ID_NATURAL_MODE.to_string(),
                label: "Natural Mode".to_string(),
                checked: false,
            }
        );
        assert_eq!(
            menu_model(&MenuInput {
                natural_mode: true,
                ..base(&[])
            })[4],
            MenuNode::Check {
                id: ID_NATURAL_MODE.to_string(),
                label: "Natural Mode".to_string(),
                checked: true,
            }
        );
    }

    #[test]
    fn settings_carries_the_platform_comma_accelerator() {
        assert_eq!(
            menu_model(&base(&[]))[5],
            MenuNode::Item {
                id: ID_SETTINGS.to_string(),
                label: "Settings".to_string(),
                enabled: true,
                accelerator: Some("CmdOrCtrl+,"),
            }
        );
    }

    #[test]
    fn quit_is_preceded_by_a_separator_and_is_last() {
        let nodes = menu_model(&base(&[]));
        assert_eq!(nodes[nodes.len() - 2], MenuNode::Separator);
        assert_eq!(
            nodes[nodes.len() - 1],
            MenuNode::Item {
                id: ID_QUIT.to_string(),
                label: "Quit Wispr Lightning".to_string(),
                enabled: true,
                accelerator: None,
            }
        );
    }

    #[test]
    fn every_clickable_id_is_unique() {
        fn collect(nodes: &[MenuNode], out: &mut Vec<String>) {
            for node in nodes {
                match node {
                    MenuNode::Item { id, .. } | MenuNode::Check { id, .. } => out.push(id.clone()),
                    MenuNode::Submenu { children, .. } => collect(children, out),
                    MenuNode::Separator => {}
                }
            }
        }

        let devices = devices();
        let nodes = menu_model(&MenuInput {
            last_transcription: Some("x"),
            ..base(&devices)
        });
        let mut ids = Vec::new();
        collect(&nodes, &mut ids);
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "duplicate menu ids in {ids:?}");
    }
}
