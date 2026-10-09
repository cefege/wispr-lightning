use std::sync::Arc;

use dioxus::prelude::*;
use wl_shell::{ops, state::AppState};

use crate::ui::{store::SettingsStore, widgets::SettingRow};

#[component]
pub fn view() -> Element {
    let state = use_context::<Arc<AppState>>();
    let store = use_context::<SettingsStore>();
    let settings = store.value.read().clone();
    let packs = use_signal(Vec::<String>::new);
    let mut preview_error = use_signal(|| None::<String>);
    let load_state = Arc::clone(&state);

    use_effect(move || {
        let state = Arc::clone(&load_state);
        let mut packs = packs;
        spawn(async move {
            if let Ok(Ok(found)) =
                tokio::task::spawn_blocking(move || ops::sound_packs(&state)).await
            {
                packs.set(found);
            }
        });
    });

    let options = packs()
        .into_iter()
        .filter(|pack| pack != "default")
        .collect::<Vec<_>>();
    let dock_title = if cfg!(target_os = "macos") {
        Some("Show in Dock")
    } else if cfg!(target_os = "windows") {
        Some("Show in taskbar")
    } else {
        None
    };
    let preview_state = Arc::clone(&state);
    let preview_error_for_click = preview_error;
    let store_for_preview = store;

    rsx! {
        div { class: "mx-auto max-w-[760px] space-y-4",
            div { class: "space-y-3 rounded-lg border border-line bg-surface p-4 shadow-card",
                h2 { class: "mb-2 text-sm font-semibold", "System" }
                SettingRow {
                    title: "Launch at login",
                    control: rsx! {
                        crate::components::switch::Switch {
                            checked: settings.launch_at_login,
                            on_checked_change: move |enabled| {
                                let mut store = store;
                                store.update(|settings| settings.launch_at_login = enabled);
                            }
                        }
                    }
                }
                if let Some(title) = dock_title {
                    SettingRow {
                        title,
                        control: rsx! {
                            crate::components::switch::Switch {
                                checked: settings.show_in_dock,
                                on_checked_change: move |enabled| {
                                    let mut store = store;
                                    store.update(|settings| settings.show_in_dock = enabled);
                                }
                            }
                        }
                    }
                }
                SettingRow {
                    title: "Sound effects",
                    control: rsx! {
                        crate::components::switch::Switch {
                            checked: settings.enable_sounds,
                            on_checked_change: move |enabled| {
                                let mut store = store;
                                store.update(|settings| settings.enable_sounds = enabled);
                            }
                        }
                    }
                }
                SettingRow {
                    title: "Mute music while dictating",
                    control: rsx! {
                        crate::components::switch::Switch {
                            checked: settings.mute_music,
                            on_checked_change: move |enabled| {
                                let mut store = store;
                                store.update(|settings| settings.mute_music = enabled);
                            }
                        }
                    }
                }
                hr { class: "my-2 border-line" }
                SettingRow {
                    title: "Verbose logging",
                    description: Some(format!(
                        "Log full server requests and responses to {}",
                        wl_core::paths::log_file().display()
                    )),
                    control: rsx! {
                        crate::components::switch::Switch {
                            checked: settings.verbose_logging,
                            on_checked_change: move |enabled| {
                                let mut store = store;
                                store.update(|settings| settings.verbose_logging = enabled);
                            }
                        }
                    }
                }
                hr { class: "my-2 border-line" }
                div { class: "flex flex-wrap items-center gap-2",
                    label { class: "mr-auto", "Sound pack" }
                    // `selected` per option: the packs load after first render.
                    select {
                        class: "h-9 rounded-md border border-line bg-control px-2",
                        onchange: move |event| {
                            let pack = event.value();
                            let mut store = store;
                            store.update(|settings| {
                                settings.selected_sound_pack =
                                    if pack.is_empty() { None } else { Some(pack) };
                            });
                        },
                        option { value: "", selected: settings.selected_sound_pack.is_none(), "Default" }
                        for pack in options.iter() {
                            option { value: "{pack}", selected: settings.selected_sound_pack.as_deref() == Some(pack.as_str()), "{capitalize(pack)}" }
                        }
                    }
                    crate::components::button::Button {
                        variant: crate::components::button::ButtonVariant::Secondary,
                        onclick: move |_| {
                            let mut store = store_for_preview;
                            let state = Arc::clone(&preview_state);
                            let pack = store.value.read().selected_sound_pack.clone();
                            let mut error = preview_error_for_click;
                            error.set(None);
                            spawn(async move {
                                if let Err(message) = store.flush().await {
                                    error.set(Some(message));
                                    return;
                                }
                                match tokio::task::spawn_blocking(move || {
                                    ops::sound_preview(&state, pack, "start")
                                })
                                .await
                                {
                                    Ok(Ok(())) => {}
                                    Ok(Err(message)) => error.set(Some(message.to_string())),
                                    Err(join_error) => error.set(Some(join_error.to_string())),
                                }
                            });
                        },
                        "Preview"
                    }
                }
                if let Some(message) = preview_error() {
                    p { class: "text-sm text-danger", role: "alert", "{message}" }
                }
                if let Some(message) = (store.save_error)() {
                    p { class: "text-sm text-danger", role: "alert", "Could not save settings: {message}" }
                }
                hr { class: "my-2 border-line" }
                SettingRow {
                    title: "First-run setup",
                    description: Some(
                        "Walk through permissions, the dictation key and Deepgram setup again."
                            .into(),
                    ),
                    control: rsx! {
                        crate::components::button::Button {
                            variant: crate::components::button::ButtonVariant::Secondary,
                            onclick: move |_| {
                                let mut store = store;
                                spawn(async move {
                                    if let Err(message) = store.restart_onboarding().await {
                                        preview_error.set(Some(message));
                                    }
                                });
                            },
                            "Run Setup Again"
                        }
                    }
                }
            }
            p { class: "text-xs text-fg-subtle", {format!("Wispr Lightning v{}", env!("CARGO_PKG_VERSION"))} }
        }
    }
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}
