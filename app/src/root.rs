//! The root component, running in the overlay window's VirtualDom for the life
//! of the process. It owns everything that must live on the main thread: the
//! tray icon, the queue of tray updates, and window creation.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use dioxus::desktop::{use_tray_icon_event_handler, use_tray_menu_event_handler};
use dioxus::prelude::*;

use wl_shell::state::AppState;

use crate::bus::Bus;
use crate::overlay::Overlay;
use crate::tray::Tray;

#[component]
pub fn Root() -> Element {
    let state = use_context::<Arc<AppState>>();
    let bus = use_context::<Arc<Bus>>();

    let tray = use_hook(|| match Tray::create(Arc::clone(&state)) {
        Ok(tray) => Some(Rc::new(RefCell::new(tray))),
        Err(e) => {
            tracing::error!(error = %e, "could not create the tray icon");
            None
        }
    });

    {
        let tray = tray.clone();
        let bus = Arc::clone(&bus);
        use_tray_menu_event_handler(move |event| {
            if let Some(tray) = &tray {
                tray.borrow().on_menu_event(&bus, event.id().as_ref());
            }
        });
    }
    {
        let tray = tray.clone();
        let bus = Arc::clone(&bus);
        use_tray_icon_event_handler(move |event| {
            if let Some(tray) = &tray {
                tray.borrow().on_icon_event(&bus, event);
            }
        });
    }

    // Main-thread queues: tray updates from the core, window requests from
    // the tray, setup and (later) the screens.
    use_future(move || {
        let receivers = bus.take_receivers();
        let tray = tray.clone();
        let state = Arc::clone(&state);
        let bus = Arc::clone(&bus);
        async move {
            let Some(mut receivers) = receivers else {
                return;
            };
            let mut show_in_dock = bus.show_in_dock.subscribe();
            crate::windows::apply_show_in_dock(*show_in_dock.borrow_and_update());
            loop {
                tokio::select! {
                    Some(command) = receivers.tray.recv() => {
                        if let Some(tray) = &tray {
                            tray.borrow_mut().apply(command);
                        }
                    }
                    Some(section) = receivers.open_main.recv() => {
                        crate::windows::open(section, Arc::clone(&state), Arc::clone(&bus)).await;
                    }
                    Ok(()) = show_in_dock.changed() => {
                        crate::windows::apply_show_in_dock(*show_in_dock.borrow_and_update());
                    }
                    else => return,
                }
            }
        }
    });

    rsx! { Overlay {} }
}
