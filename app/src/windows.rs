//! The app's ordinary windows: settings, history, dictionary and notes.
//!
//! Each is created on demand and then kept: closing hides it, so reopening is
//! instant and the view state survives (SET-008). Screens are compiled in per
//! cargo feature until the cutover; opening one that is not built logs and
//! does nothing.
//!
//! Covers MATRIX SET-001..SET-009 and WIN-001.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use dioxus::desktop::tao::event::{Event, WindowEvent};
use dioxus::desktop::{
    use_wry_event_handler, window, Config, LogicalPosition, LogicalSize, WeakDesktopContext,
    WindowBuilder, WindowCloseBehaviour,
};
use dioxus::prelude::*;

use wl_shell::state::AppState;

use crate::bus::Bus;
use crate::window_state::{self, Geometry};

/// History, Dictionary and Notes join with their screens (Steps 3-5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowName {
    Settings,
}

/// Geometry and chrome for one window.
struct Spec {
    label: &'static str,
    title: &'static str,
    width: f64,
    height: f64,
    min_width: f64,
    min_height: f64,
}

impl WindowName {
    fn spec(self) -> Spec {
        match self {
            // SET-001 / SET-002 / SET-004: the Swift content rect and minimum,
            // and the title verbatim.
            Self::Settings => Spec {
                label: "settings",
                title: "Wispr Lightning Settings",
                width: 860.0,
                height: 580.0,
                min_width: 680.0,
                min_height: 460.0,
            },
        }
    }

    /// The screen this window renders, if it is built into this binary.
    fn screen(self) -> Option<fn() -> Element> {
        match self {
            // Screens land here one cargo feature at a time (Steps 2-5).
            Self::Settings => None,
        }
    }
}

thread_local! {
    /// Live managed windows. Main thread only: desktop contexts are `!Send`.
    static WINDOWS: RefCell<HashMap<WindowName, WeakDesktopContext>> = RefCell::new(HashMap::new());
}

/// Show `name`, creating it the first time and reusing it afterwards. Must be
/// called from a component task on the main thread.
pub async fn open(name: WindowName, state: Arc<AppState>, bus: Arc<Bus>) {
    let existing = WINDOWS.with_borrow(|w| w.get(&name).and_then(|weak| weak.upgrade()));
    if let Some(context) = existing {
        context.window.set_visible(true);
        context.window.set_minimized(false);
        context.window.set_focus();
        activate_app();
        return;
    }

    if name.screen().is_none() {
        tracing::warn!(?name, "screen is not built into this binary");
        return;
    }

    let spec = name.spec();
    let show_in_dock = *bus.show_in_dock.borrow();
    let dom = VirtualDom::new_with_props(ManagedWindow, ManagedWindowProps { name })
        .with_root_context(state)
        .with_root_context(bus);
    let context = window().new_window(dom, config(&spec, show_in_dock)).await;
    WINDOWS.with_borrow_mut(|w| w.insert(name, std::rc::Rc::downgrade(&context)));
    activate_app();
}

fn config(spec: &Spec, show_in_dock: bool) -> Config {
    let mut builder = WindowBuilder::new()
        .with_title(spec.title)
        .with_inner_size(LogicalSize::new(spec.width, spec.height))
        .with_min_inner_size(LogicalSize::new(spec.min_width, spec.min_height))
        .with_resizable(true);

    // SET-006 / SET-007: a remembered frame is restored; otherwise tao centres
    // a window that has no explicit position.
    if let Some(saved) = window_state::load(spec.label) {
        builder = builder
            .with_inner_size(LogicalSize::new(saved.width, saved.height))
            .with_position(LogicalPosition::new(saved.x, saved.y));
    }

    // A tray-first app has no taskbar entry unless "Show in Dock" is on
    // (LIF-010).
    #[cfg(target_os = "windows")]
    {
        use dioxus::desktop::tao::platform::windows::WindowBuilderExtWindows;
        builder = builder.with_skip_taskbar(!show_in_dock);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = show_in_dock;

    Config::new()
        .with_window(builder)
        .with_menu(None)
        .with_close_behaviour(WindowCloseBehaviour::WindowHides)
        .with_disable_context_menu(true)
        .with_data_directory(wl_core::paths::app_support_dir().join("webview"))
        .with_custom_head(format!(
            "<style>{}</style>",
            include_str!("../assets/app.css")
        ))
}

/// Apply "Show in Dock" to every live managed window (Windows: taskbar entry).
/// macOS: the activation policy. Main thread only.
pub fn apply_show_in_dock(show: bool) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
        if let Some(mtm) = objc2_foundation::MainThreadMarker::new() {
            let policy = if show {
                NSApplicationActivationPolicy::Regular
            } else {
                NSApplicationActivationPolicy::Accessory
            };
            NSApplication::sharedApplication(mtm).setActivationPolicy(policy);
        }
    }

    #[cfg(target_os = "windows")]
    {
        use dioxus::desktop::tao::platform::windows::WindowExtWindows;
        WINDOWS.with_borrow(|windows| {
            for (name, weak) in windows {
                if let Some(context) = weak.upgrade() {
                    if let Err(e) = context.window.set_skip_taskbar(!show) {
                        tracing::warn!(error = %e, ?name, "could not change the taskbar entry");
                    }
                }
            }
        });
    }
}

/// Bring the app forward so a newly shown window is not buried (SET-009). An
/// accessory app is not activated by ordering a window front.
#[cfg(target_os = "macos")]
fn activate_app() {
    let Some(mtm) = objc2_foundation::MainThreadMarker::new() else {
        return;
    };
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
}

/// Showing a window already brings it to the front on Windows.
#[cfg(not(target_os = "macos"))]
fn activate_app() {}

#[derive(Props, Clone, PartialEq)]
struct ManagedWindowProps {
    name: WindowName,
}

/// Root of every managed window: geometry persistence, the platform attribute
/// and accent colour the stylesheet keys off, then the screen itself.
#[allow(non_snake_case)]
fn ManagedWindow(props: ManagedWindowProps) -> Element {
    let label = props.name.spec().label;
    let bus = use_context::<Arc<Bus>>();
    let mut geometry = use_signal(|| None::<Geometry>);

    use_hook(|| {
        let platform = if cfg!(target_os = "macos") {
            "macos"
        } else {
            "windows"
        };
        document::eval(&format!(
            "document.documentElement.dataset.platform = '{platform}';"
        ));
    });

    use_future(move || {
        let mut accent = bus.accent.subscribe();
        async move {
            loop {
                let current = accent.borrow_and_update().clone();
                if let Some(accent) = current {
                    crate::accent::apply(&accent);
                }
                if accent.changed().await.is_err() {
                    return;
                }
            }
        }
    });

    // Remember the frame as it changes, and write it out when the window
    // loses focus or closes: a drag fires dozens of moves per second.
    use_wry_event_handler(move |event, _| {
        let Event::WindowEvent {
            window_id, event, ..
        } = event
        else {
            return;
        };
        let desktop = window();
        if *window_id != desktop.window.id() {
            return;
        }
        match event {
            WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
                let scale = desktop.window.scale_factor();
                let (Ok(position), size) =
                    (desktop.window.outer_position(), desktop.window.inner_size())
                else {
                    return;
                };
                let position = position.to_logical::<f64>(scale);
                let size = size.to_logical::<f64>(scale);
                geometry.set(Some(Geometry {
                    x: position.x,
                    y: position.y,
                    width: size.width,
                    height: size.height,
                }));
            }
            WindowEvent::Focused(false) | WindowEvent::CloseRequested => {
                if let Some(frame) = *geometry.peek() {
                    window_state::save(label, frame);
                }
            }
            _ => {}
        }
    });

    // `open` only creates windows whose screen is built in.
    props
        .name
        .screen()
        .map_or_else(|| rsx! {}, |screen| screen())
}
