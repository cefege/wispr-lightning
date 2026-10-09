use std::sync::Arc;

use dioxus::prelude::*;
use wl_shell::{ops, state::AppState};

use crate::ui::{store::SettingsStore, widgets::{ErrorBanner, SettingRow}};

#[component]
pub fn view() -> Element {
    let state = use_context::<Arc<AppState>>();
    let store = use_context::<SettingsStore>();
    let value = store.value.read().clone();
    let status = use_signal(|| None::<ops::DeepgramStatus>);
    let load_error = use_signal(|| None::<String>);
    let mut api_key = use_signal(String::new);
    let key_busy = use_signal(|| false);
    let key_error = use_signal(|| None::<String>);
    let health = use_signal(|| None::<ops::DeepgramHealth>);
    let balance = use_signal(|| None::<ops::DeepgramBalance>);
    let balance_error = use_signal(|| None::<String>);
    let balance_busy = use_signal(|| false);
    let health_busy = use_signal(|| false);

    let mount_state = state.clone();
    use_effect(move || refresh_status(mount_state.clone(), status, load_error));
    let configured = status().map(|s| s.configured).unwrap_or(false);
    let supports_nova2 = value.deepgram_model.to_lowercase().starts_with("nova-2");
    let lang_options = crate::ui::format::language_options(&value.deepgram_model, &value.deepgram_language);
    let unsupported_name = if value.deepgram_language != "__auto__" && value.deepgram_language != "__multi__" {
        wl_providers::languages::LANGUAGES.iter().find(|l| l.code == value.deepgram_language && supports_nova2 && !l.nova2).map(|l| l.name)
    } else { None };
    let retry_state = state.clone();
    let enter_save_state = state.clone();
    let button_save_state = state.clone();
    let clear_state = state.clone();
    let balance_state = state.clone();
    let health_state = state.clone();
    rsx! {
        div { class: "mx-auto max-w-[760px] space-y-4",
            div { class: "space-y-3 rounded-lg border border-line bg-surface p-4 shadow-card",
                div { class: "flex items-start justify-between gap-3",
                    div { strong { "Live streaming transcription" } p { class: "mt-1 text-xs text-fg-muted", "Audio is sent directly to Deepgram while you dictate." } }
                    span { class: if configured { "text-xs text-success" } else { "text-xs text-danger" }, if configured { "API key saved" } else { "API key required" } }
                }
                if let Some(err) = load_error() { ErrorBanner { message: err, on_retry: Some(EventHandler::new(move |_| refresh_status(retry_state.clone(), status, load_error))) } }
                hr { class: "border-line" }
                div { class: "space-y-2",
                    label { "API key" }
                    div { class: "flex flex-wrap items-center gap-2",
                        input { class: "h-9 min-w-64 flex-1 rounded-md border border-line bg-control px-3", r#type: "password", value: "{api_key()}", placeholder: if configured { "Saved key ••••••••••••" } else { "Paste Deepgram API key" }, autocomplete: "off", oninput: move |e| api_key.set(e.value()), onkeydown: move |e| if e.key() == Key::Enter { save_key(enter_save_state.clone(), api_key(), KeySaveSignals { key: api_key, busy: key_busy, error: key_error, health, status, load_error }); } }
                        crate::components::button::Button { disabled: key_busy() || api_key().trim().is_empty(), onclick: move |_| save_key(button_save_state.clone(), api_key(), KeySaveSignals { key: api_key, busy: key_busy, error: key_error, health, status, load_error }), if configured { "Replace" } else { "Save key" } }
                        if configured { crate::components::button::Button { variant: crate::components::button::ButtonVariant::Secondary, disabled: key_busy(), onclick: move |_| clear_key(clear_state.clone(), api_key, key_busy, key_error, health, status, load_error), "Clear" } }
                    }
                    p { class: "m-0 text-xs text-fg-muted", "Stored locally in the app data folder. The saved value is never revealed." }
                    if let Some(err) = key_error() { p { class: "text-sm text-danger", role: "alert", "{err}" } }
                }
                if configured {
                    hr { class: "border-line" }
                    div { class: "flex items-center justify-between gap-3",
                        div { strong { "Credits remaining" } p { class: "m-0 text-xs text-fg-muted", if let Some(b) = balance() { "{b.project_name}" } else { "Prepaid balance on your Deepgram account." } } }
                        div { class: "flex items-center gap-2",
                            if let Some(b) = balance() { span { class: "font-semibold", "{crate::ui::format::balance(b.amount, b.units.as_str())}" } }
                            crate::components::button::Button { variant: crate::components::button::ButtonVariant::Secondary, disabled: balance_busy(), onclick: move |_| load_balance(balance_state.clone(), balance, balance_error, balance_busy), if balance_busy() { "Checking…" } else if balance().is_some() { "Refresh" } else { "Check" } }
                        }
                    }
                    if let Some(err) = balance_error() { p { class: "text-sm text-danger", role: "alert", "{err}" } }
                }
                hr { class: "border-line" }
                div { class: "flex items-center justify-between gap-4", label { "Model" }
                    select { class: "h-9 rounded-md border border-line bg-control px-2", value: "{value.deepgram_model}", onchange: move |e| { let model = e.value(); let mut s = store; s.update(|s| s.deepgram_model = model); },
                        option { value: "nova-3", "Nova 3 — recommended" } option { value: "nova-2", "Nova 2" }
                    }
                }
                div { class: "flex items-center justify-between gap-4", label { "Language" }
                    select { class: "h-9 max-w-[360px] rounded-md border border-line bg-control px-2", value: if value.deepgram_language == "__multi__" { "__auto__".to_string() } else { value.deepgram_language.clone() }, onchange: move |e| { let language = e.value(); let mut s = store; s.update(|s| s.deepgram_language = language); },
                        for option in lang_options { option { value: "{option.value}", "{option.label}" } }
                    }
                }
                if let Some(name) = unsupported_name { p { class: "text-sm text-danger", role: "alert", "Nova 2 does not accept {name}. Dictation will fail until you pick another language or switch back to Nova 3." } }
                p { class: "text-xs text-fg-muted", "Auto-detect streams Deepgram's multilingual model, which recognises several languages in one dictation; its per-language coverage is narrower than picking a fixed language." }
                SettingRow { title: "Contextual recognition hints", description: Some(if value.deepgram_model.to_lowercase().starts_with("nova-3") { "Send dictionary terms and distinctive words from the focused app. Screen OCR is included when enabled in Privacy." } else { "Nova 3 is required. The current model will not receive dictionary or context keyterms." }.into()), control: rsx! { crate::components::switch::Switch { checked: value.deepgram_keyterm_boost, disabled: !value.deepgram_model.to_lowercase().starts_with("nova-3"), on_checked_change: move |enabled| { let mut s = store; s.update(|s| s.deepgram_keyterm_boost = enabled); } } } }
                hr { class: "border-line" }
                div { class: "flex items-center gap-3",
                    crate::components::button::Button { variant: crate::components::button::ButtonVariant::Secondary, disabled: health_busy() || !configured, onclick: move |_| test_connection(health_state.clone(), health, health_busy), if health_busy() { "Testing…" } else { "Test connection" } }
                    if let Some(h) = health() { span { class: if h.ok { "text-sm text-success" } else { "text-sm text-danger" }, "{h.message}" } }
                }
            }
            div { class: "rounded-lg border border-line bg-surface p-4 shadow-card",
                h2 { class: "mb-2 text-sm font-semibold", "Transcript processing" }
                p { class: "m-0 text-sm text-fg-muted", "Deepgram returns recognizer text. Wispr Lightning then applies your replacements and snippets locally, followed by capitalization and terminal punctuation." }
            }
        }
    }
}

fn refresh_status(state: Arc<AppState>, mut status: Signal<Option<ops::DeepgramStatus>>, mut error: Signal<Option<String>>) {
    spawn(async move { match tokio::task::spawn_blocking(move || ops::deepgram_status(&state)).await {
        Ok(Ok(value)) => { status.set(Some(value)); error.set(None); },
        Ok(Err(e)) => error.set(Some(e.to_string())), Err(e) => error.set(Some(e.to_string())),
    }});
}
#[derive(Clone, Copy)]
struct KeySaveSignals {
    key: Signal<String>,
    busy: Signal<bool>,
    error: Signal<Option<String>>,
    health: Signal<Option<ops::DeepgramHealth>>,
    status: Signal<Option<ops::DeepgramStatus>>,
    load_error: Signal<Option<String>>,
}

fn save_key(state: Arc<AppState>, key: String, mut signals: KeySaveSignals) {
    if key.trim().is_empty() { return; }
    signals.busy.set(true);
    signals.error.set(None);
    spawn(async move {
        let request_state = state.clone();
        let raw = key.clone();
        let result = tokio::task::spawn_blocking(move || ops::deepgram_key_save(&request_state, &raw)).await;
        match result {
            Ok(Ok(())) => {
                signals.key.set(String::new());
                signals.health.set(None);
                refresh_status(state, signals.status, signals.load_error);
            }
            Ok(Err(error)) => signals.error.set(Some(error.to_string())),
            Err(error) => signals.error.set(Some(error.to_string())),
        }
        signals.busy.set(false);
    });
}
fn clear_key(state: Arc<AppState>, mut key: Signal<String>, mut busy: Signal<bool>, mut error: Signal<Option<String>>, mut health: Signal<Option<ops::DeepgramHealth>>, status: Signal<Option<ops::DeepgramStatus>>, load_error: Signal<Option<String>>) {
    busy.set(true); error.set(None);
    spawn(async move { let request_state = state.clone(); let result = tokio::task::spawn_blocking(move || ops::deepgram_key_clear(&request_state)).await;
        match result { Ok(Ok(())) => { key.set(String::new()); health.set(None); refresh_status(state, status, load_error); }, Ok(Err(e)) => error.set(Some(e.to_string())), Err(e) => error.set(Some(e.to_string())) }
        busy.set(false);
    });
}
fn load_balance(state: Arc<AppState>, mut balance: Signal<Option<ops::DeepgramBalance>>, mut error: Signal<Option<String>>, mut busy: Signal<bool>) {
    busy.set(true); error.set(None);
    spawn(async move { match ops::deepgram_balance(&state).await { Ok(value) => balance.set(Some(value)), Err(e) => { balance.set(None); error.set(Some(e.to_string())); } } busy.set(false); });
}
fn test_connection(state: Arc<AppState>, mut health: Signal<Option<ops::DeepgramHealth>>, mut busy: Signal<bool>) {
    busy.set(true);
    spawn(async move { match ops::deepgram_health(&state).await { Ok(value) => health.set(Some(value)), Err(e) => health.set(Some(ops::DeepgramHealth { ok: false, message: e.to_string() })) } busy.set(false); });
}
