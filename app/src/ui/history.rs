use std::sync::Arc;

use chrono::{Local, TimeZone};
use dioxus::prelude::*;
use wl_core::db::models::TranscriptEntry;
use wl_shell::ops;
use wl_shell::state::AppState;

use crate::bus::{Bus, HISTORY_CHANGED};
use crate::ui::{
    copy_text, format,
    icons::{Icon, IconName},
    widgets::{EmptyState, ErrorBanner, SearchField},
};

const PAGE: i64 = 50;

#[component]
pub fn view() -> Element {
    let state = use_context::<Arc<AppState>>();
    let bus = use_context::<Arc<Bus>>();
    let mut entries = use_signal(Vec::<TranscriptEntry>::new);
    let mut query = use_signal(String::new);
    let mut loading = use_signal(|| true);
    let mut loading_more = use_signal(|| false);
    let mut has_more = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut generation = use_signal(|| 0_u64);
    let mut pending_delete = use_signal(|| None::<TranscriptEntry>);
    let mut confirming_clear = use_signal(|| false);

    let load_state = state.clone();
    let load = use_callback(move |()| {
        loading_more.set(false);
        // `peek`: the mount effect calls this, and subscribing it to the
        // signals written here would re-run it forever.
        let stamp = *generation.peek() + 1;
        generation.set(stamp);
        loading.set(true);
        let search = query.peek().trim().to_owned();
        let unfiltered = search.is_empty();
        let state = load_state.clone();
        spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                if search.is_empty() {
                    ops::history_list(&state, PAGE, 0)
                } else {
                    ops::history_search(&state, &search)
                }
            })
            .await
            .map_err(|e| e.to_string())
            .and_then(|result| result);
            if generation() != stamp {
                return;
            }
            match result {
                Ok(rows) => {
                    has_more.set(unfiltered && rows.len() == PAGE as usize);
                    entries.set(rows);
                    error.set(None);
                }
                Err(message) => error.set(Some(message)),
            }
            loading.set(false);
        });
    });

    let refresh_state = state.clone();
    let refresh = use_callback(move |()| {
        if !query().trim().is_empty() {
            load.call(());
            return;
        }
        let stamp = generation() + 1;
        generation.set(stamp);
        let limit = (entries().len() as i64).max(PAGE);
        let state = refresh_state.clone();
        spawn(async move {
            let result = tokio::task::spawn_blocking(move || ops::history_list(&state, limit, 0))
                .await
                .map_err(|e| e.to_string())
                .and_then(|result| result);
            if generation() != stamp {
                return;
            }
            match result {
                Ok(rows) => {
                    entries.set(rows);
                    error.set(None);
                }
                Err(message) => error.set(Some(message)),
            }
        });
    });

    let load_more_state = state.clone();
    let load_more = use_callback(move |()| {
        if loading_more() || !has_more() {
            return;
        }
        let stamp = generation();
        let offset = entries().len() as i64;
        let state = load_more_state.clone();
        loading_more.set(true);
        spawn(async move {
            let result =
                tokio::task::spawn_blocking(move || ops::history_list(&state, PAGE, offset))
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|result| result);
            if generation() == stamp {
                match result {
                    Ok(rows) => {
                        has_more.set(rows.len() == PAGE as usize);
                        entries.with_mut(|old| old.extend(rows));
                        error.set(None);
                    }
                    Err(message) => error.set(Some(message)),
                }
                loading_more.set(false);
            }
        });
    });

    use_effect(move || load.call(()));

    let refresh_event = refresh;
    use_future(move || {
        let mut changes = bus.changed.subscribe();
        async move {
            loop {
                match changes.recv().await {
                    Ok(topic) if topic == HISTORY_CHANGED => refresh_event.call(()),
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
        }
    });
    let refresh_focus = refresh;
    crate::ui::use_on_focus(move || refresh_focus.call(()));

    let groups = format::group_by_day(entries(), Local::now());

    let delete_state = state.clone();
    let delete = use_callback(move |()| {
        let Some(entry) = pending_delete() else {
            return;
        };
        pending_delete.set(None);
        let state = delete_state.clone();
        spawn(async move {
            let result =
                tokio::task::spawn_blocking(move || ops::history_delete(&state, &entry.id))
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|result| result);
            match result {
                Ok(()) => refresh.call(()),
                Err(message) => error.set(Some(message)),
            }
        });
    });
    let clear_state = state.clone();
    let clear = use_callback(move |()| {
        confirming_clear.set(false);
        let state = clear_state.clone();
        spawn(async move {
            let result = tokio::task::spawn_blocking(move || ops::history_clear(&state))
                .await
                .map_err(|e| e.to_string())
                .and_then(|result| result);
            match result {
                Ok(()) => load.call(()),
                Err(message) => error.set(Some(message)),
            }
        });
    });

    rsx! {
        div { class: "flex h-full min-h-0 flex-col gap-4",
            div { class: "flex items-center gap-3",
                SearchField {
                    value: query(),
                    placeholder: "Search",
                    oninput: move |event: FormEvent| { query.set(event.value()); load.call(()); }
                }
                if !entries().is_empty() {
                    crate::components::button::Button {
                        variant: crate::components::button::ButtonVariant::Destructive,
                        onclick: move |_| confirming_clear.set(true),
                        "Clear All"
                    }
                }
            }
            if !entries().is_empty() {
                if let Some(message) = error() {
                    ErrorBanner {
                        message,
                        on_retry: Some(EventHandler::new(move |_| load.call(())))
                    }
                }
            }
            div {
                class: "min-h-0 flex-1 space-y-3 overflow-y-auto",
                onscroll: move |event| {
                    let data = event.data();
                    let remaining = data.scroll_height() as f64 - data.scroll_top() - data.client_height() as f64;
                    if remaining < data.client_height() as f64 {
                        load_more.call(());
                    }
                },
                if loading() && entries().is_empty() {
                    EmptyState { icon: IconName::HistoryEmpty, title: "Loading…" }
                } else if entries().is_empty() && error().is_some() {
                    EmptyState {
                        icon: IconName::Warning,
                        title: "Couldn't load history",
                        action_label: Some("Retry".to_string()),
                        on_action: Some(EventHandler::new(move |_| load.call(())))
                    }
                    p { class: "text-center text-sm text-danger", "{error().unwrap_or_default()}" }
                } else if entries().is_empty() {
                    EmptyState { icon: IconName::HistoryEmpty, title: "No dictations yet" }
                } else {
                    for (title, group) in groups {
                        h2 { class: "sticky top-0 z-10 mb-2 mt-4 bg-window/95 py-1 text-xs font-semibold uppercase tracking-wide text-fg-muted first:mt-0", "{title}" }
                        for entry in group {
                            {
                                let copy_entry = entry.clone();
                                let delete_entry = entry.clone();
                                rsx! {
                                    article { class: "rounded-lg border border-line bg-surface p-4 shadow-card",
                                        div { class: "mb-2 flex items-center gap-2 text-xs text-fg-muted",
                                            span { {Local.timestamp_opt(entry.timestamp as i64, 0).single().map(|time| time.format("%-I:%M %p").to_string()).unwrap_or_default()} }
                                            span { "·" }
                                            span { "{entry.app_name}" }
                                            span { "·" }
                                            span { "{entry.duration_secs:.1}s" }
                                            span { "·" }
                                            span { "{entry.num_words} words" }
                                            span { class: "flex-1" }
                                            crate::components::button::Button {
                                                variant: crate::components::button::ButtonVariant::Ghost,
                                                size: crate::components::button::ButtonSize::Icon,
                                                title: "Copy", "aria-label": "Copy",
                                                onclick: move |_| {
                                                    let text = copy_entry.formatted_text.clone()
                                                        .or(copy_entry.asr_text.clone())
                                                        .unwrap_or_default();
                                                    if let Err(message) = copy_text(&text) {
                                                        let mut error = error;
                                                        error.set(Some(format!("Could not copy to the clipboard: {message}")));
                                                    }
                                                },
                                                Icon { name: IconName::Copy }
                                            }
                                            crate::components::button::Button {
                                                variant: crate::components::button::ButtonVariant::Ghost,
                                                size: crate::components::button::ButtonSize::Icon,
                                                title: "Delete", "aria-label": "Delete",
                                                onclick: move |_| {
                                                    let mut pending_delete = pending_delete;
                                                    pending_delete.set(Some(delete_entry.clone()));
                                                },
                                                Icon { name: IconName::Trash }
                                            }
                                        }
                                        p { class: "m-0 whitespace-pre-wrap text-[13px] leading-relaxed text-fg", "{entry.formatted_text.clone().or(entry.asr_text.clone()).unwrap_or_default()}" }
                                    }
                                }
                            }
                        }
                    }
                    if has_more() {
                        div { class: "flex justify-center py-2",
                            crate::components::button::Button {
                                variant: crate::components::button::ButtonVariant::Secondary,
                                disabled: loading_more(),
                                onclick: move |_| load_more.call(()),
                                if loading_more() { "Loading…" } else { "Load More" }
                            }
                        }
                    }
                }
            }
        }
        if pending_delete().is_some() {
            div { class: "fixed inset-0 z-50 grid place-items-center bg-black/40",
                role: "dialog", "aria-modal": "true",
                onkeydown: move |event| {
                    if event.key() == Key::Escape {
                        pending_delete.set(None);
                    } else if event.key() == Key::Enter {
                        event.prevent_default();
                        delete.call(());
                    }
                },
                div { class: "w-[min(420px,calc(100vw-32px))] rounded-xl border border-line bg-surface p-5 shadow-modal", onclick: move |event| event.stop_propagation(),
                    h2 { class: "m-0 text-base font-semibold", "Delete this entry?" }
                    p { class: "mt-2 text-sm text-fg-muted", "This action cannot be undone." }
                    div { class: "mt-5 flex justify-end gap-2",
                        crate::components::button::Button { variant: crate::components::button::ButtonVariant::Secondary, onclick: move |_| pending_delete.set(None), "Cancel" }
                        crate::components::button::Button { variant: crate::components::button::ButtonVariant::Destructive, onclick: move |_| delete.call(()), "Delete" }
                    }
                }
            }
        }
        if confirming_clear() {
            div { class: "fixed inset-0 z-50 grid place-items-center bg-black/40",
                role: "dialog", "aria-modal": "true",
                onkeydown: move |event| {
                    if event.key() == Key::Escape {
                        confirming_clear.set(false);
                    } else if event.key() == Key::Enter {
                        event.prevent_default();
                        clear.call(());
                    }
                },
                div { class: "w-[min(420px,calc(100vw-32px))] rounded-xl border border-line bg-surface p-5 shadow-modal", onclick: move |event| event.stop_propagation(),
                    h2 { class: "m-0 text-base font-semibold", "Clear all history?" }
                    p { class: "mt-2 text-sm text-fg-muted", "This will delete all transcript entries. This action cannot be undone." }
                    div { class: "mt-5 flex justify-end gap-2",
                        crate::components::button::Button { variant: crate::components::button::ButtonVariant::Secondary, onclick: move |_| confirming_clear.set(false), "Cancel" }
                        crate::components::button::Button { variant: crate::components::button::ButtonVariant::Destructive, onclick: move |_| clear.call(()), "Clear All" }
                    }
                }
            }
        }
    }
}
