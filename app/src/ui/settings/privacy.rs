use std::sync::Arc;
use dioxus::prelude::*;
use wl_shell::{ops, state::AppState};

use crate::ui::{store::SettingsStore, widgets::{ErrorBanner, SettingRow}};

#[component]
pub fn view() -> Element {
    let state = use_context::<Arc<AppState>>();
    let store = use_context::<SettingsStore>();
    let value = store.value.read().clone();
    let permissions = use_signal(Vec::<(String, String)>::new);
    let error = use_signal(|| None::<String>);
    let loaded = use_signal(|| false);
    let mount_state = state.clone();
    use_effect(move || refresh_permissions(mount_state.clone(), permissions, error, loaded));
    let retry_state = state.clone();
    let rows_state = state.clone();
    let entries = permissions();
    rsx! {
        div { class: "mx-auto max-w-[760px] space-y-4",
            div { class: "rounded-lg border border-line bg-surface p-4 shadow-card",
                h2 { class: "mb-2 text-sm font-semibold", "Privacy" }
                SettingRow { title: "Screen context (OCR)", description: Some("Capture screen text for context-aware formatting".into()), control: rsx! { crate::components::switch::Switch { checked: value.use_screen_context, on_checked_change: move |v| { let mut s = store; s.update(|s| s.use_screen_context = v); } } } }
                SettingRow { title: "Accessibility context", description: Some("Use accessibility APIs for better transcription context".into()), control: rsx! { crate::components::switch::Switch { checked: value.use_accessibility_context, on_checked_change: move |v| { let mut s = store; s.update(|s| s.use_accessibility_context = v); } } } }
                SettingRow { title: "Share anonymous usage data", description: Some("Help improve Wispr by sharing anonymous statistics".into()), control: rsx! { crate::components::switch::Switch { checked: value.share_usage_data, on_checked_change: move |v| { let mut s = store; s.update(|s| s.share_usage_data = v); } } } }
            }
            div { class: "space-y-3 rounded-lg border border-line bg-surface p-4 shadow-card",
                h2 { class: "m-0 text-sm font-semibold", "System Permissions" }
                p { class: "m-0 text-xs text-fg-muted", "Dictation needs these to hear you, to see the key you press, and to type into the app in front of you. A denial here is silent otherwise — the app simply never triggers." }
                if let Some(message) = error() { ErrorBanner { message, on_retry: Some(EventHandler::new(move |_| refresh_permissions(retry_state.clone(), permissions, error, loaded))) } }
                else if entries.is_empty() { p { class: "text-xs text-fg-muted", if loaded() { "No permissions to configure." } else { "Checking…" } } }
                else { ul { class: "m-0 list-none divide-y divide-line p-0",
                    for (key, status) in entries.iter() {
                        {
                            let permission_key = key.clone();
                            let display = permission_label(key);
                            let state_label = permission_state_label(status);
                            let action = match status.as_str() {
                                "not_determined" => Some((false, "Request Access")),
                                "denied" => Some((true, "Open Settings")),
                                _ => None,
                            };
                            let state = rows_state.clone();
                            rsx! {
                                li { class: "flex min-h-12 items-center gap-3 py-2",
                                    span { class: "flex-1 text-[13px] font-medium", "{display}" }
                                    span { class: "text-xs text-fg-muted", "{state_label}" }
                                    if let Some((open, label)) = action {
                                        crate::components::button::Button {
                                            variant: crate::components::button::ButtonVariant::Secondary,
                                            onclick: move |_| request_permission(
                                                state.clone(),
                                                permission_key.clone(),
                                                open,
                                                permissions,
                                                error,
                                                loaded,
                                            ),
                                            "{label}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                } }
            }
        }
    }
}

fn refresh_permissions(state: Arc<AppState>, mut permissions: Signal<Vec<(String, String)>>, mut error: Signal<Option<String>>, mut loaded: Signal<bool>) {
    spawn(async move {
        match tokio::task::spawn_blocking(move || ops::permissions_status(&state)).await {
            Ok(Ok(map)) => { permissions.set(map.into_iter().map(|(k, v)| (k, v.to_string())).collect()); error.set(None); }
            Ok(Err(e)) => error.set(Some(e.to_string())), Err(e) => error.set(Some(e.to_string())),
        }
        loaded.set(true);
    });
}
fn request_permission(state: Arc<AppState>, key: String, open: bool, permissions: Signal<Vec<(String, String)>>, mut error: Signal<Option<String>>, loaded: Signal<bool>) {
    spawn(async move {
        let request_state = state.clone();
        let result = tokio::task::spawn_blocking(move || if open { ops::permissions_open_settings(&request_state, &key) } else { ops::permissions_request(&request_state, &key) }).await;
        match result { Ok(Ok(())) => error.set(None), Ok(Err(e)) => error.set(Some(e.to_string())), Err(e) => error.set(Some(e.to_string())) }
        refresh_permissions(state, permissions, error, loaded);
    });
}
fn permission_label(key: &str) -> String {
    match key {
        "microphone" => "Microphone".into(),
        "accessibility" => "Accessibility".into(),
        "input_monitoring" => "Input Monitoring".into(),
        "screen_recording" => "Screen Recording".into(),
        "automation" => "Automation".into(),
        _ => key
            .split('_')
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}
fn permission_state_label(state: &str) -> &'static str { match state { "granted" => "Granted", "denied" => "Denied", "not_determined" => "Not requested", "not_applicable" => "Not required on this system", _ => "Not required on this system" } }
