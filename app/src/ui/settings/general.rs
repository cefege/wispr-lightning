use std::{sync::Arc, time::Duration};

use dioxus::prelude::*;
use wl_shell::{ops, state::AppState};

use crate::{
    bus::{Bus, DEVICES_CHANGED},
    ui::{
        store::SettingsStore,
        widgets::{ErrorBanner, KeyCapture, SettingRow},
    },
};

#[component]
pub fn view() -> Element {
    let state = use_context::<Arc<AppState>>();
    let bus = use_context::<Arc<Bus>>();
    let store = use_context::<SettingsStore>();
    let settings = store.value.read().clone();
    let devices = use_signal(Vec::<ops::InputDeviceInfo>::new);
    let device_error = use_signal(|| None::<String>);
    let mut capturing = use_signal(|| None::<usize>);
    let mut capture_started = use_signal(|| None::<std::time::Instant>);
    let cleanup_state = state.clone();
    use_drop(move || cleanup_state.cancel_hotkey_capture());

    let device_state = state.clone();
    use_effect(move || refresh_devices(device_state.clone(), devices, device_error));
    let event_state = state.clone();
    let event_bus = Arc::clone(&bus);
    use_future(move || {
        let event_state = Arc::clone(&event_state);
        let mut events = event_bus.changed.subscribe();
        async move {
            loop {
                if matches!(events.recv().await, Ok(DEVICES_CHANGED)) {
                    refresh_devices(event_state.clone(), devices, device_error);
                }
            }
        }
    });
    let capture_state = state.clone();
    use_future(move || {
        let capture_state = Arc::clone(&capture_state);
        let mut store = store;
        let mut capturing = capturing;
        let mut capture_started = capture_started;
        async move {
            loop {
                if capturing().is_some() {
                    if let Some(key) = capture_state.end_hotkey_capture() {
                        if !store.value.read().hotkeys.contains(&key) {
                            store.update(|settings| settings.hotkeys.push(key));
                        }
                        capturing.set(None);
                        capture_started.set(None);
                    } else if capture_started()
                        .is_some_and(|started| started.elapsed() >= Duration::from_secs(15))
                    {
                        capture_state.cancel_hotkey_capture();
                        capturing.set(None);
                        capture_started.set(None);
                    }
                }
                tokio::time::sleep(Duration::from_millis(120)).await;
            }
        }
    });

    let press_behavior = settings.hotkey_press_behavior.clone();
    let note = match press_behavior.as_str() {
        "hold" => "Recording lasts as long as the key is held; releasing always ends it. A quick tap stops recording immediately.",
        "toggle" => "A quick tap starts recording hands-free and the next press stops it. Holding the key is still push-to-talk.",
        _ => "A quick tap keeps recording until half a second after the press and then stops; tapping again inside that window locks hands-free. Holding the key is still push-to-talk.",
    };
    let retry_state = state.clone();
    let refresh_state = state.clone();
    let key_state = state.clone();
    let add_state = state.clone();
    rsx! {
        div { class: "mx-auto max-w-[760px] space-y-4",
            div { class: "rounded-lg border border-line bg-surface p-4 shadow-card space-y-3",
                h2 { class: "m-0 text-sm font-semibold", "Dictation Hotkeys" }
                p { class: "m-0 text-sm text-fg-muted", "Any of these keys will start dictation:" }
                for (index, hotkey) in settings.hotkeys.iter().enumerate() {
                    div { class: "flex items-center gap-2",
                        KeyCapture { label: hotkey.label(), capturing: capturing() == Some(index), onclick: {
                            let key_state = key_state.clone();
                            move |_| {
                                if capturing() == Some(index) { key_state.cancel_hotkey_capture(); capturing.set(None); capture_started.set(None); }
                                else { key_state.begin_hotkey_capture(); capture_started.set(Some(std::time::Instant::now())); capturing.set(Some(index)); }
                            }
                        } }
                        if settings.hotkeys.len() > 1 {
                            crate::components::button::Button { variant: crate::components::button::ButtonVariant::Ghost,
                                onclick: move |_| { let mut s = store; s.update(|s| if s.hotkeys.len() > 1 { s.hotkeys.remove(index); }); }, "Remove" }
                        }
                    }
                }
                if capturing().is_some() { p { class: "m-0 text-xs text-fg-muted", "Press the key you want. For a combination, hold the modifiers and press the other key. Click again to cancel." } }
                crate::components::button::Button { variant: crate::components::button::ButtonVariant::Secondary, onclick: move |_| {
                    capturing.set(Some(settings.hotkeys.len())); capture_started.set(Some(std::time::Instant::now())); add_state.begin_hotkey_capture();
                }, "Add Hotkey" }
                p { class: "m-0 text-xs text-fg-subtle", "Modifier keys work as hold-to-talk. Regular keys use press-to-toggle." }
                SettingRow { title: "Press behavior", description: Some(note.into()), control: rsx! {
                    select { class: "h-9 rounded-md border border-line bg-control px-2", value: "{settings.hotkey_press_behavior}", onchange: move |e| { let v=e.value(); if ["hold","toggle","legacy"].contains(&v.as_str()) { let mut s=store; s.update(|s| s.hotkey_press_behavior=v); } },
                        option { value: "hold", "Hold to talk" } option { value: "toggle", "Tap to start, tap to stop" } option { value: "legacy", "Hold or double-tap to lock" }
                    }
                } }
            }
            div { class: "rounded-lg border border-line bg-surface p-4 shadow-card space-y-3",
                h2 { class: "m-0 text-sm font-semibold", "Input Device" }
                if let Some(message) = device_error() { ErrorBanner { message, on_retry: Some(EventHandler::new(move |_| refresh_devices(retry_state.clone(), devices, device_error))) } }
                div { class: "flex items-center gap-2",
                    // `selected` per option, not `value` on the select: the
                    // device list arrives after the first render, and a value
                    // set before its option exists is not re-applied.
                    select { class: "h-9 min-w-56 rounded-md border border-line bg-control px-2", onchange: move |e| { let id=e.value(); let name=devices.read().iter().find(|d| d.id==id).map(|d| d.name.clone()); let mut s=store; s.update(|s| {s.mic_device_id=if id.is_empty(){None}else{Some(id)};s.mic_device_name=name;}); },
                        option { value: "", selected: settings.mic_device_id.is_none(), "System Default" }
                        for d in devices.read().iter() { option { value: "{d.id}", selected: settings.mic_device_id.as_deref() == Some(d.id.as_str()), "{d.name}" } }
                    }
                    crate::components::button::Button { variant: crate::components::button::ButtonVariant::Secondary, onclick: move |_| refresh_devices(refresh_state.clone(), devices, device_error), "Refresh" }
                }
                SettingRow { title: "Keep microphone active", control: rsx! { crate::components::switch::Switch { checked: settings.keep_microphone_active, on_checked_change: move |v| {let mut s=store;s.update(|s|s.keep_microphone_active=v);} } } }
            }
        }
    }
}
fn refresh_devices(
    state: Arc<AppState>,
    mut devices: Signal<Vec<ops::InputDeviceInfo>>,
    mut error: Signal<Option<String>>,
) {
    spawn(async move {
        match tokio::task::spawn_blocking(move || ops::audio_devices(&state)).await {
            Ok(Ok(found)) => {
                devices.set(found);
                error.set(None);
            }
            Ok(Err(e)) => error.set(Some(e.to_string())),
            Err(e) => error.set(Some(e.to_string())),
        }
    });
}
