//! A portable hotkey representation.
//!
//! The Swift app stored macOS Carbon virtual keycodes (`59` = Left Control)
//! directly in `settings.json`. Those are meaningless on Windows, so the model
//! is side-specific modifier flags plus an optional non-modifier key, and the
//! legacy keycodes are translated once on load.
//!
//! Side-specific matters: Left Control and Right Control are independent
//! triggers. Collapsing them would make two user-configured alternatives
//! indistinguishable.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A set of side-specific modifier keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Modifiers(u32);

macro_rules! modifiers {
    ($($konst:ident = $bit:expr, $name:literal, $mac:literal, $win:literal;)*) => {
        impl Modifiers {
            $(pub const $konst: Modifiers = Modifiers(1 << $bit);)*

            const ALL: &'static [(Modifiers, &'static str, &'static str, &'static str)] = &[
                $((Modifiers(1 << $bit), $name, $mac, $win),)*
            ];
        }
    };
}

modifiers! {
    CTRL_LEFT   = 0, "ctrl_left",   "Left Control",  "Left Ctrl";
    CTRL_RIGHT  = 1, "ctrl_right",  "Right Control", "Right Ctrl";
    ALT_LEFT    = 2, "alt_left",    "Left Option",   "Left Alt";
    ALT_RIGHT   = 3, "alt_right",   "Right Option",  "Right Alt";
    META_LEFT   = 4, "meta_left",   "Left Command",  "Left Win";
    META_RIGHT  = 5, "meta_right",  "Right Command", "Right Win";
    SHIFT_LEFT  = 6, "shift_left",  "Left Shift",    "Left Shift";
    SHIFT_RIGHT = 7, "shift_right", "Right Shift",   "Right Shift";
    FN          = 8, "fn",          "Fn",            "Fn";
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers(0);

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn contains(self, other: Modifiers) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn bits(self) -> u32 {
        self.0
    }

    pub fn from_bits_truncate(bits: u32) -> Self {
        let mask = Self::ALL.iter().fold(0, |acc, (m, ..)| acc | m.0);
        Self(bits & mask)
    }

    /// Individual flags set, in declaration order.
    pub fn iter(self) -> impl Iterator<Item = Modifiers> {
        Self::ALL
            .iter()
            .map(|(m, ..)| *m)
            .filter(move |m| self.contains(*m))
    }

    fn names(self) -> Vec<&'static str> {
        Self::ALL
            .iter()
            .filter(|(m, ..)| self.contains(*m))
            .map(|(_, n, ..)| *n)
            .collect()
    }

    fn from_name(name: &str) -> Option<Modifiers> {
        Self::ALL
            .iter()
            .find(|(_, n, ..)| *n == name)
            .map(|(m, ..)| *m)
    }

    /// Human label using the platform's own vocabulary: macOS says "Option"
    /// and "Command", Windows says "Alt" and "Win".
    pub fn label(self) -> String {
        Self::ALL
            .iter()
            .filter(|(m, ..)| self.contains(*m))
            .map(|(_, _, mac, win)| {
                if cfg!(target_os = "macos") {
                    *mac
                } else {
                    *win
                }
            })
            .collect::<Vec<_>>()
            .join(" + ")
    }
}

impl std::ops::BitOr for Modifiers {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for Modifiers {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl Serialize for Modifiers {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.names().serialize(s)
    }
}

impl<'de> Deserialize<'de> for Modifiers {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let names = Vec::<String>::deserialize(d)?;
        // Unknown names are ignored rather than fatal: a settings file written
        // by a newer build must not brick an older one.
        Ok(names
            .iter()
            .filter_map(|n| Modifiers::from_name(n))
            .fold(Modifiers::NONE, |acc, m| acc | m))
    }
}

/// A non-modifier key that can act as a trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerKey {
    Return,
    Space,
    Escape,
    Tab,
    F(u8),
}

impl TriggerKey {
    pub fn label(self) -> String {
        match self {
            Self::Return => "Return".into(),
            Self::Space => "Space".into(),
            Self::Escape => "Escape".into(),
            Self::Tab => "Tab".into(),
            Self::F(n) => format!("F{n}"),
        }
    }
}

/// One trigger. `key: None` means a bare modifier hold — the default and by far
/// the most common configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hotkey {
    #[serde(default)]
    pub modifiers: Modifiers,
    #[serde(default)]
    pub key: Option<TriggerKey>,
}

impl Hotkey {
    /// A bare modifier hold, e.g. push-to-talk on Left Control.
    pub fn modifier(modifiers: Modifiers) -> Self {
        Self {
            modifiers,
            key: None,
        }
    }

    pub fn combo(modifiers: Modifiers, key: TriggerKey) -> Self {
        Self {
            modifiers,
            key: Some(key),
        }
    }

    /// Whether this trigger is a bare modifier with no accompanying key.
    pub fn is_modifier_only(&self) -> bool {
        self.key.is_none() && !self.modifiers.is_empty()
    }

    /// A hotkey with no modifiers and no key can never fire.
    pub fn is_valid(&self) -> bool {
        !self.modifiers.is_empty() || self.key.is_some()
    }

    /// Translate a legacy macOS Carbon virtual keycode.
    pub fn from_carbon_keycode(code: u16) -> Option<Self> {
        let hk = match code {
            59 => Self::modifier(Modifiers::CTRL_LEFT),
            62 => Self::modifier(Modifiers::CTRL_RIGHT),
            58 => Self::modifier(Modifiers::ALT_LEFT),
            61 => Self::modifier(Modifiers::ALT_RIGHT),
            55 => Self::modifier(Modifiers::META_LEFT),
            54 => Self::modifier(Modifiers::META_RIGHT),
            56 => Self::modifier(Modifiers::SHIFT_LEFT),
            60 => Self::modifier(Modifiers::SHIFT_RIGHT),
            63 => Self::modifier(Modifiers::FN),
            36 => Self::combo(Modifiers::NONE, TriggerKey::Return),
            49 => Self::combo(Modifiers::NONE, TriggerKey::Space),
            53 => Self::combo(Modifiers::NONE, TriggerKey::Escape),
            48 => Self::combo(Modifiers::NONE, TriggerKey::Tab),
            _ => return None,
        };
        Some(hk)
    }

    /// Display string for the settings UI and the tray tooltip.
    pub fn label(&self) -> String {
        match (self.modifiers.is_empty(), self.key) {
            (true, Some(k)) => k.label(),
            (false, Some(k)) => format!("{} + {}", self.modifiers.label(), k.label()),
            (false, None) => self.modifiers.label(),
            (true, None) => "Unset".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_and_right_modifiers_are_distinct() {
        assert_ne!(Modifiers::CTRL_LEFT, Modifiers::CTRL_RIGHT);
        assert!(!Modifiers::CTRL_LEFT.contains(Modifiers::CTRL_RIGHT));
        // This is the property rdev loses: users can bind either side as an
        // independent dictation trigger.
        let both = Modifiers::CTRL_LEFT | Modifiers::CTRL_RIGHT;
        assert!(both.contains(Modifiers::CTRL_LEFT));
        assert!(both.contains(Modifiers::CTRL_RIGHT));
        assert_eq!(both.iter().count(), 2);
    }

    #[test]
    fn empty_modifiers_contain_nothing_but_are_trivially_self_containing() {
        assert!(Modifiers::NONE.is_empty());
        assert!(!Modifiers::NONE.contains(Modifiers::FN));
        assert!(Modifiers::CTRL_LEFT.contains(Modifiers::NONE));
    }

    #[test]
    fn carbon_keycodes_map_to_the_documented_keys() {
        let cases = [
            (59u16, Modifiers::CTRL_LEFT),
            (62, Modifiers::CTRL_RIGHT),
            (58, Modifiers::ALT_LEFT),
            (61, Modifiers::ALT_RIGHT),
            (55, Modifiers::META_LEFT),
            (54, Modifiers::META_RIGHT),
            (56, Modifiers::SHIFT_LEFT),
            (60, Modifiers::SHIFT_RIGHT),
            (63, Modifiers::FN),
        ];
        for (code, expected) in cases {
            assert_eq!(
                Hotkey::from_carbon_keycode(code),
                Some(Hotkey::modifier(expected)),
                "keycode {code}"
            );
        }
        assert_eq!(
            Hotkey::from_carbon_keycode(49),
            Some(Hotkey::combo(Modifiers::NONE, TriggerKey::Space))
        );
    }

    #[test]
    fn unknown_carbon_keycodes_are_rejected_rather_than_guessed() {
        assert_eq!(Hotkey::from_carbon_keycode(0), None);
        assert_eq!(Hotkey::from_carbon_keycode(9999), None);
    }

    #[test]
    fn modifier_only_hotkeys_are_representable() {
        let hk = Hotkey::modifier(Modifiers::CTRL_LEFT);
        assert!(hk.is_modifier_only());
        assert!(hk.is_valid());
        assert_eq!(hk.key, None);
    }

    #[test]
    fn an_empty_hotkey_is_invalid() {
        let hk = Hotkey {
            modifiers: Modifiers::NONE,
            key: None,
        };
        assert!(!hk.is_valid());
        assert!(!hk.is_modifier_only());
        assert_eq!(hk.label(), "Unset");
    }

    #[test]
    fn round_trips_through_json_as_readable_names() {
        let hk = Hotkey::combo(
            Modifiers::CTRL_LEFT | Modifiers::SHIFT_LEFT,
            TriggerKey::Space,
        );
        let json = serde_json::to_string(&hk).unwrap();
        assert!(json.contains("ctrl_left"), "{json}");
        assert!(json.contains("shift_left"), "{json}");
        assert_eq!(serde_json::from_str::<Hotkey>(&json).unwrap(), hk);
    }

    #[test]
    fn unknown_modifier_names_are_ignored_not_fatal() {
        // A settings file from a newer build must not brick an older one.
        let hk: Hotkey =
            serde_json::from_str(r#"{"modifiers":["ctrl_left","hyper_key"],"key":null}"#).unwrap();
        assert_eq!(hk, Hotkey::modifier(Modifiers::CTRL_LEFT));
    }

    #[test]
    fn a_hotkey_with_omitted_fields_deserializes_to_unset() {
        let hk: Hotkey = serde_json::from_str("{}").unwrap();
        assert!(!hk.is_valid());
    }

    #[test]
    fn labels_use_platform_vocabulary() {
        let label = Modifiers::ALT_LEFT.label();
        if cfg!(target_os = "macos") {
            assert_eq!(label, "Left Option");
            assert_eq!(Modifiers::META_LEFT.label(), "Left Command");
        } else {
            assert_eq!(label, "Left Alt");
            assert_eq!(Modifiers::META_LEFT.label(), "Left Win");
        }
    }

    #[test]
    fn combo_labels_join_modifiers_and_key() {
        let hk = Hotkey::combo(Modifiers::SHIFT_LEFT, TriggerKey::Space);
        assert_eq!(hk.label(), "Left Shift + Space");
        assert_eq!(
            Hotkey::combo(Modifiers::NONE, TriggerKey::Tab).label(),
            "Tab"
        );
        assert_eq!(TriggerKey::F(5).label(), "F5");
    }

    #[test]
    fn bits_round_trip_and_reject_junk() {
        let m = Modifiers::CTRL_LEFT | Modifiers::FN;
        assert_eq!(Modifiers::from_bits_truncate(m.bits()), m);
        assert_eq!(Modifiers::from_bits_truncate(0xFFFF_FFFF).iter().count(), 9);
    }
}
