//! The system accent colour, written into the CSS custom properties the
//! stylesheet reads. Webviews answer the CSS `AccentColor` keyword with a
//! hardcoded blue, so the value comes from the platform observer instead.

use dioxus::prelude::document;
use wl_shell::ops::AccentColor;

/// Set the four accent properties on the current window's root element.
pub fn apply(accent: &AccentColor) {
    // Values are `#rrggbb` hex from `Rgb::to_hex`, so no quoting is needed
    // beyond the JSON string encoding.
    let set = |name: &str, value: &str| {
        format!(
            "s.setProperty('{name}', {});",
            serde_json::Value::String(value.to_string())
        )
    };
    document::eval(&format!(
        "const s = document.documentElement.style; {}{}{}{}",
        set("--accent", &accent.accent),
        set("--accent-text", &accent.text),
        set("--accent-darker", &accent.darker),
        set("--accent-lighter", &accent.lighter),
    ));
}
