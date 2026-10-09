//! The recording overlay: a 36 px pill, bottom-centre of the primary work
//! area, that must never take focus from the app being dictated into.
//!
//! The overlay is the root Dioxus window. It exists from launch (OVL-041, so
//! the first hotkey press pays no construction latency) and is only ever shown
//! and hidden. Every visual difference between states is a CSS rule keyed off
//! `data-state`; this module writes that attribute, two strings, a width and
//! eighteen bar heights, and sizes the window to match.
//!
//! Covers MATRIX OVL-001..OVL-043 on the shell side.

mod view;
mod vu;

use std::sync::Arc;

use dioxus::desktop::tao::window::Window;
use dioxus::desktop::{
    window, Config, LogicalPosition, LogicalSize, WindowBuilder, WindowCloseBehaviour,
};
use dioxus::prelude::*;

use wl_shell::overlay_geometry::{
    accepts_clicks, overlay_frame, width_for, INITIAL_WIDTH, OVERLAY_HEIGHT,
};
use wl_shell::state::AppState;
use wl_shell::ui::OverlayState;

use crate::bus::Bus;
use vu::{LevelMeter, BAR_COUNT};

/// Window configuration for the root (overlay) window.
pub fn window_config() -> Config {
    let builder = WindowBuilder::new()
        .with_title("Wispr Lightning")
        // THE invariant: never focusable, never focused.
        .with_focusable(false)
        .with_focused(false)
        .with_always_on_top(true)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(false)
        .with_maximizable(false)
        .with_minimizable(false)
        .with_closable(false)
        .with_visible(false)
        .with_inner_size(LogicalSize::new(INITIAL_WIDTH, OVERLAY_HEIGHT));

    #[cfg(target_os = "windows")]
    let builder = {
        use dioxus::desktop::tao::platform::windows::WindowBuilderExtWindows;
        builder
            .with_skip_taskbar(true)
            // Suppresses the white flash a transparent window shows before
            // its first paint.
            .with_no_redirection_bitmap(true)
            // The shadow is drawn in CSS around the pill's rounded outline; a
            // window shadow would outline the transparent bounding box.
            .with_undecorated_shadow(false)
    };

    #[cfg(target_os = "macos")]
    let builder = {
        use dioxus::desktop::tao::platform::macos::WindowBuilderExtMacOS;
        builder.with_has_shadow(false)
    };

    Config::new()
        .with_window(builder)
        .with_background_color((0, 0, 0, 0))
        .with_menu(None)
        .with_disable_context_menu(true)
        .with_close_behaviour(WindowCloseBehaviour::WindowHides)
        .with_exits_when_last_window_closes(false)
        // The default left-click handler would show this window.
        .with_tray_icon_show_window_on_click(false)
        .with_data_directory(wl_core::paths::app_support_dir().join("webview"))
        .with_custom_head(format!(
            "<style>{}</style><style>{}</style><style>{}</style>",
            include_str!("../../assets/app.css"),
            include_str!("../../assets/overlay.css"),
            // Dioxus mounts into `#main`; let the pill be laid out as if it
            // were the body's direct child, as overlay.css expects.
            "html, body { margin: 0; height: 100%; } #main { display: contents; }",
        ))
        .with_on_window(|window, _dom| {
            crate::platform::harden_overlay(&window);
            // Passive states are click-through, and the overlay starts passive.
            if let Err(e) = window.set_ignore_cursor_events(true) {
                tracing::warn!(error = %e, "could not make the overlay click-through");
            }
        })
}

/// Size, place, and show or hide the window for `state`.
///
/// Repositions only when the width changes (and always after a hide), so the
/// 25 Hz-adjacent elapsed updates cost nothing (OVL-018).
fn present(window: &Window, current_width: &mut Option<f64>, state: &OverlayState, elapsed: bool) {
    let Some(width) = width_for(state, elapsed) else {
        window.set_visible(false);
        *current_width = None;
        return;
    };
    if *current_width != Some(width) {
        match crate::platform::work_area(window) {
            Some(area) => {
                let frame = overlay_frame(area, width);
                window.set_inner_size(LogicalSize::new(frame.width, frame.height));
                window.set_outer_position(LogicalPosition::new(frame.x, frame.y));
                *current_width = Some(width);
            }
            None => tracing::warn!("no monitor to position the overlay on"),
        }
    }
    if let Err(e) = window.set_ignore_cursor_events(!accepts_clicks(state)) {
        tracing::warn!(error = %e, "could not change overlay click-through");
    }
    // Non-activating: WS_EX_NOACTIVATE (Windows) is set on the window.
    window.set_visible(true);
}

#[component]
pub fn Overlay() -> Element {
    let state = use_context::<Arc<AppState>>();
    let bus = use_context::<Arc<Bus>>();

    let mut overlay = use_signal(|| OverlayState::Hidden);
    let mut elapsed_label = use_signal(|| None::<String>);
    let mut warning = use_signal(|| 0u8);
    let mut bars = use_signal(|| [vu::BAR_MIN_HEIGHT; BAR_COUNT]);
    let mut saved = use_signal(|| false);
    let meter = use_hook(|| std::rc::Rc::new(std::cell::RefCell::new(LevelMeter::default())));
    let width = use_hook(|| std::rc::Rc::new(std::cell::Cell::new(None::<f64>)));

    // Window geometry follows the state and the elapsed readout's presence.
    {
        let width = std::rc::Rc::clone(&width);
        use_effect(move || {
            let current = overlay();
            let elapsed = elapsed_label().is_some() && view::is_recording(&current);
            let mut w = width.get();
            present(&window().window, &mut w, &current, elapsed);
            width.set(w);
        });
    }

    {
        let bus = Arc::clone(&bus);
        let meter = std::rc::Rc::clone(&meter);
        use_future(move || {
            let mut rx = bus.overlay_state.subscribe();
            let meter = std::rc::Rc::clone(&meter);
            async move {
                while rx.changed().await.is_ok() {
                    let next = rx.borrow_and_update().clone();
                    let previous = overlay.peek().clone();
                    let changed = view::key(&next) != view::key(&previous);
                    if view::resets_warning(&next) {
                        warning.set(0);
                    }
                    if changed {
                        // A fresh presentation: timer hidden, Save offerable again
                        // (OVL-019, OVL-038). Recording -> Locked keeps the band
                        // flowing; every other entry returns it to silence.
                        elapsed_label.set(None);
                        saved.set(false);
                        if !(view::is_recording(&previous) && view::is_recording(&next)) {
                            meter.borrow_mut().reset();
                            bars.set(*meter.borrow().heights());
                        }
                    }
                    overlay.set(next);
                }
            }
        });
    }

    {
        let bus = Arc::clone(&bus);
        use_future(move || {
            let mut rx = bus.elapsed.subscribe();
            async move {
                while rx.changed().await.is_ok() {
                    let next = rx.borrow_and_update().clone();
                    elapsed_label.set(next.label);
                    // Monotonic within a recording; only a state transition
                    // steps it back down (OVL-034).
                    let current = *warning.peek();
                    warning.set(current.max(next.warning));
                }
            }
        });
    }

    {
        let bus = Arc::clone(&bus);
        let meter = std::rc::Rc::clone(&meter);
        use_future(move || {
            let mut rx = bus.level.subscribe();
            let meter = std::rc::Rc::clone(&meter);
            async move {
                while rx.changed().await.is_ok() {
                    let level = *rx.borrow_and_update();
                    // A level that lands after the microphone closed is
                    // dropped: buffering it would flash a stale bar on the
                    // next show.
                    if !view::is_recording(&overlay.peek()) {
                        continue;
                    }
                    meter.borrow_mut().push(level);
                    let heights = *meter.borrow().heights();
                    if heights != *bars.peek() {
                        bars.set(heights);
                    }
                }
            }
        });
    }

    let current = overlay();
    let recording = view::is_recording(&current);
    let elapsed = elapsed_label().filter(|_| recording);
    let pill_width = width_for(&current, elapsed.is_some()).unwrap_or(INITIAL_WIDTH);
    let action = move |name: &'static str| {
        let state = Arc::clone(&state);
        move |_| {
            if let Err(e) = wl_shell::ops::overlay_action(&state, name) {
                tracing::warn!(error = %e, action = name, "overlay action failed");
            }
        }
    };
    let on_save = {
        let save = action("save");
        move |e| {
            save(e);
            // Confirm in place and refuse a second write (OVL-036).
            saved.set(true);
        }
    };

    rsx! {
        div {
            id: "pill",
            class: "pill",
            "data-state": view::key(&current),
            "data-warning": "{warning}",
            style: "width: {pill_width}px",
            span { id: "vu", class: "vu", "aria-hidden": "true",
                for height in bars() {
                    i { class: "vu-bar", style: "height: {height}px" }
                }
            }
            svg { id: "spinner", class: "spinner", view_box: "0 0 16 16", "aria-hidden": "true",
                circle { class: "spinner-track", cx: "8", cy: "8", r: "6.5" }
                circle { class: "spinner-arc", cx: "8", cy: "8", r: "6.5" }
            }
            span { id: "label", class: "label", {view::label(&current)} }
            span { id: "time", class: "time", {elapsed.unwrap_or_default()} }
            button { id: "retry", class: "btn btn-bezel", r#type: "button",
                onmousedown: |e| e.prevent_default(),
                onclick: action("retry"),
                "Retry"
            }
            button { id: "save", class: "btn btn-bezel", r#type: "button",
                disabled: saved(),
                onmousedown: |e| e.prevent_default(),
                onclick: on_save,
                if saved() { "Saved" } else { "Save" }
            }
            button { id: "dismiss", class: "btn btn-inline", r#type: "button",
                "aria-label": "Dismiss",
                onmousedown: |e| e.prevent_default(),
                onclick: action("dismiss"),
                "\u{2715}"
            }
            button { id: "cancel", class: "cancel", r#type: "button",
                title: "Cancel recording",
                "aria-label": "Cancel recording",
                onmousedown: |e| e.prevent_default(),
                onclick: action("cancel"),
                svg { view_box: "0 0 20 20", "aria-hidden": "true",
                    mask { id: "cancel-xmark",
                        circle { cx: "10", cy: "10", r: "9", fill: "#fff" }
                        path {
                            d: "M6.6 6.6 13.4 13.4M13.4 6.6 6.6 13.4",
                            stroke: "#000",
                            stroke_width: "1.8",
                            stroke_linecap: "round",
                        }
                    }
                    circle { cx: "10", cy: "10", r: "9", fill: "currentColor", mask: "url(#cancel-xmark)" }
                }
            }
        }
    }
}
