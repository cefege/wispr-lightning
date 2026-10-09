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

static TAILWIND: Asset = asset!("/assets/tailwind.css");
static DX_THEME: Asset = asset!("/assets/dx-components-theme.css");

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
        .with_custom_head(
            "<style>html, body { margin: 0; height: 100%; background: transparent !important; overflow: hidden !important; } body { display: flex; align-items: center; justify-content: center; -webkit-app-region: no-drag; app-region: no-drag; } #main { display: contents; }</style>".to_string(),
        )
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
    let current_warning = *warning.read();
    let (tint, tint_alpha) = match &current {
        OverlayState::Error { .. } | OverlayState::Recoverable { .. } => ("var(--danger)", 0.3),
        OverlayState::Retrying { .. } => ("var(--warning)", 0.2),
        _ if current_warning == 1 => ("var(--warning)", 0.3),
        _ if current_warning == 2 => ("var(--danger)", 0.3),
        _ => ("transparent", 0.0),
    };
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
        document::Stylesheet { href: TAILWIND }
        document::Stylesheet { href: DX_THEME }
        div {
            id: "pill",
            class: "group relative flex h-9 items-center justify-center gap-2 overflow-hidden rounded-pill px-4 shadow-pop backdrop-blur-[30px] [backdrop-filter:saturate(180%)] before:absolute before:inset-0 before:z-0 before:pointer-events-none before:bg-elevated before:opacity-[0.72] before:content-[''] after:absolute after:inset-0 after:z-0 after:pointer-events-none after:bg-[var(--tint)] after:opacity-[var(--tint-alpha)] after:content-[''] [&>*]:relative [&>*]:z-10 data-[state=hidden]:invisible",
            "data-state": view::key(&current),
            "data-warning": "{current_warning}",
            style: format!(
                "width: {}px; --tint: {}; --tint-alpha: {}",
                pill_width, tint, tint_alpha
            ),
            span { id: "vu", class: "hidden h-[22px] w-[88px] flex-none items-end gap-[2px] group-data-[state=recording]:flex group-data-[state=locked]:flex", "aria-hidden": "true",
                for height in bars() {
                    i { class: "w-[3px] rounded-[1.5px] bg-recording transition-[height] duration-[60ms] ease-linear group-data-[state=locked]:bg-success", style: "height: {height}px" }
                }
            }
            svg { id: "spinner", class: "hidden size-4 flex-none text-fg-muted group-data-[state=processing]:block group-data-[state=inserting]:block group-data-[state=retrying]:block", view_box: "0 0 16 16", fill: "none", stroke: "currentColor", stroke_width: "2", "aria-hidden": "true",
                circle { class: "opacity-25", cx: "8", cy: "8", r: "6.5" }
                circle { class: "origin-center animate-wl-spin", cx: "8", cy: "8", r: "6.5", stroke_dasharray: "10.2 30.6", stroke_linecap: "round" }
            }
            span { id: "label", class: "min-w-0 flex-1 truncate whitespace-nowrap text-[13px] text-fg empty:hidden", {view::label(&current)} }
            span { id: "time", class: "flex-none whitespace-nowrap text-[13px] tabular-nums text-fg-muted empty:hidden", {elapsed.unwrap_or_default()} }
            button { id: "retry", class: "hidden h-5 flex-none items-center rounded-sm border border-line bg-control px-2.5 text-[11px] text-fg shadow-sm hover:bg-control-hover active:bg-selected group-data-[state=recoverable]:inline-flex", r#type: "button",
                onmousedown: |e| e.prevent_default(),
                onclick: action("retry"),
                "Retry"
            }
            button { id: "save", class: "hidden h-5 flex-none items-center rounded-sm border border-line bg-control px-2.5 text-[11px] text-fg shadow-sm hover:bg-control-hover active:bg-selected group-data-[state=recoverable]:inline-flex", r#type: "button",
                disabled: saved(),
                onmousedown: |e| e.prevent_default(),
                onclick: on_save,
                if saved() { "Saved" } else { "Save" }
            }
            button { id: "dismiss", class: "hidden flex-none border-0 bg-transparent px-0.5 text-[13px] text-fg-muted hover:text-fg group-data-[state=recoverable]:inline-flex", r#type: "button",
                "aria-label": "Dismiss",
                onmousedown: |e| e.prevent_default(),
                onclick: action("dismiss"),
                "\u{2715}"
            }
            button { id: "cancel", class: "pointer-events-none absolute right-2 top-2 z-20 size-5 cursor-pointer border-0 bg-transparent p-0 text-fg-muted opacity-0 transition-opacity duration-100 group-data-[state=recording]:pointer-events-auto group-data-[state=locked]:pointer-events-auto group-hover:group-data-[state=recording]:opacity-100 group-hover:group-data-[state=locked]:opacity-100 group-focus-within:opacity-100", r#type: "button",
                title: "Cancel recording",
                "aria-label": "Cancel recording",
                onmousedown: |e| e.prevent_default(),
                onclick: action("cancel"),
                svg { class: "block size-5", view_box: "0 0 20 20", "aria-hidden": "true",
                    mask { id: "cancel-xmark",
                        circle { cx: "10", cy: "10", r: "9", fill: "#fff" }
                        path { d: "M6.6 6.6 13.4 13.4M13.4 6.6 6.6 13.4", stroke: "#000", stroke_width: "1.8", stroke_linecap: "round" }
                    }
                    circle { cx: "10", cy: "10", r: "9", fill: "currentColor", mask: "url(#cancel-xmark)" }
                }
            }
        }
    }
}
