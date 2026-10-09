//! The single managed application window and its navigation model.

use std::cell::RefCell;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Home,
    History,
    Dictionary,
    Notes,
    General,
    Dictation,
    Transcription,
    Privacy,
    System,
}

impl Section {
    pub fn title(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::History => "History",
            Self::Dictionary => "Dictionary",
            Self::Notes => "Notes",
            Self::General => "General",
            Self::Dictation => "Dictation",
            Self::Transcription => "Transcription",
            Self::Privacy => "Privacy",
            Self::System => "System",
        }
    }

    pub const NAV: [&'static [Section]; 2] = [
        &[Self::Home, Self::History, Self::Dictionary, Self::Notes],
        &[
            Self::General,
            Self::Dictation,
            Self::Transcription,
            Self::Privacy,
            Self::System,
        ],
    ];
}

thread_local! {
    /// The one live managed window. Desktop contexts are main-thread only.
    static WINDOW: RefCell<Option<WeakDesktopContext>> = const { RefCell::new(None) };
}

/// Show the main window, creating it on first use and navigating when reused.
pub async fn open(section: Option<Section>, state: Arc<AppState>, bus: Arc<Bus>) {
    let existing =
        WINDOW.with_borrow(|window| window.as_ref().and_then(WeakDesktopContext::upgrade));
    if let Some(context) = existing {
        context.window.set_visible(true);
        context.window.set_minimized(false);
        context.window.set_focus();
        activate_app();
        if let Some(section) = section {
            let _ = bus.navigate.send(section);
        }
        return;
    }

    let initial = section.unwrap_or(Section::Home);
    let show_in_dock = *bus.show_in_dock.borrow();
    let dom = VirtualDom::new_with_props(ManagedWindow, ManagedWindowProps { initial })
        .with_root_context(state)
        .with_root_context(bus);
    // Shown by `ManagedWindow` once its webview has initialised: dioxus-desktop
    // reapplies the root (overlay) window's hidden start state at that point.
    let context = window().new_window(dom, config(show_in_dock)).await;
    WINDOW.with_borrow_mut(|window| *window = Some(std::rc::Rc::downgrade(&context)));
}

fn config(show_in_dock: bool) -> Config {
    let mut builder = WindowBuilder::new()
        .with_title("Wispr Lightning")
        .with_inner_size(LogicalSize::new(980.0, 660.0))
        .with_min_inner_size(LogicalSize::new(780.0, 520.0))
        .with_resizable(true);

    if let Some(saved) = window_state::load("main") {
        builder = builder
            .with_inner_size(LogicalSize::new(saved.width, saved.height))
            .with_position(LogicalPosition::new(saved.x, saved.y));
    }

    #[cfg(target_os = "windows")]
    {
        use dioxus::desktop::tao::platform::windows::WindowBuilderExtWindows;
        builder = builder.with_skip_taskbar(!show_in_dock);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = show_in_dock;

    #[cfg(target_os = "macos")]
    {
        use dioxus::desktop::tao::platform::macos::WindowBuilderExtMacOS;
        builder = builder
            .with_titlebar_transparent(true)
            .with_title_hidden(true)
            .with_fullsize_content_view(true);
    }

    Config::new()
        .with_window(builder)
        .with_menu(None)
        .with_close_behaviour(WindowCloseBehaviour::WindowHides)
        .with_disable_context_menu(true)
        .with_data_directory(wl_core::paths::app_support_dir().join("webview"))
}

/// Apply "Show in Dock" to the live main window (Windows: taskbar entry).
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
        WINDOW.with_borrow(|window| {
            if let Some(context) = window.as_ref().and_then(WeakDesktopContext::upgrade) {
                if let Err(e) = context.window.set_skip_taskbar(!show) {
                    tracing::warn!(error = %e, "could not change the taskbar entry");
                }
            }
        });
    }
}

#[cfg(target_os = "macos")]
fn activate_app() {
    let Some(mtm) = objc2_foundation::MainThreadMarker::new() else {
        return;
    };
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    app.activate();
}

#[cfg(not(target_os = "macos"))]
fn activate_app() {}

#[derive(Props, Clone, PartialEq)]
struct ManagedWindowProps {
    initial: Section,
}

#[allow(non_snake_case)]
fn ManagedWindow(props: ManagedWindowProps) -> Element {
    let mut geometry = use_signal(|| None::<Geometry>);
    let bus = use_context::<Arc<Bus>>();

    let accent = bus.accent.subscribe();
    use_future(move || {
        let mut accent = accent.clone();
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

    // dioxus-desktop 0.7 hides every new webview when it initialises if the
    // root window started hidden (the overlay does), and a hidden webview
    // never acknowledges edits, so this window's effects and tasks would never
    // run to undo it. This handler is registered during the rebuild inside
    // that initialisation, so the first event it receives is dispatched after
    // the hide: show the window there, outside the stalled VirtualDom.
    let mut shown = false;
    use_wry_event_handler(move |event, _| {
        let desktop = window();
        if !shown {
            shown = true;
            desktop.window.set_visible(true);
            desktop.window.set_minimized(false);
            desktop.window.set_focus();
            activate_app();
        }
        let Event::WindowEvent {
            window_id, event, ..
        } = event
        else {
            return;
        };
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
                    window_state::save("main", frame);
                }
            }
            _ => {}
        }
    });

    crate::ui::MainWindow(crate::ui::MainWindowProps {
        initial: props.initial,
    })
}
