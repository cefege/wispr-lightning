use std::sync::Arc;

use dioxus::prelude::*;
use wl_core::db::models::DictionaryEntry;
use wl_shell::{ops, state::AppState};

use crate::{bus::{Bus, DICTIONARY_CHANGED}, ui::{self, icons::{Icon, IconName}, widgets::{EmptyState, SearchField}}};

#[derive(Clone, PartialEq)]
struct Draft {
    entry: Option<DictionaryEntry>,
    snippet: bool,
    phrase: String,
    replacement: String,
}

#[component]
pub fn view() -> Element {
    let state = use_context::<Arc<AppState>>();
    let bus = use_context::<Arc<Bus>>();
    let mut tab = use_signal(|| false); // false = Vocabulary; true = Snippets
    let mut query = use_signal(String::new);
    let mut vocabulary = use_signal(Vec::<DictionaryEntry>::new);
    let mut snippets = use_signal(Vec::<DictionaryEntry>::new);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| None::<String>);
    let mut draft = use_signal(|| None::<Draft>);
    let mut delete = use_signal(|| None::<DictionaryEntry>);
    let mut import_result = use_signal(|| None::<String>);
    let mut menu = use_signal(|| None::<(f64, f64, DictionaryEntry)>);
    let mut generation = use_signal(|| 0_u64);

    let mut load = {
        let state = state.clone();
        move || {
            let stamp = generation() + 1;
            generation.set(stamp);
            loading.set(true);
            let state_vocabulary = state.clone();
            let state_snippets = state.clone();
            spawn(async move {
                let (vocabulary_result, snippets_result) = tokio::join!(
                    tokio::task::spawn_blocking(move || ops::dictionary_list(&state_vocabulary, "vocabulary")),
                    tokio::task::spawn_blocking(move || ops::dictionary_list(&state_snippets, "snippets"))
                );
                let result = vocabulary_result
                    .map_err(|error| error.to_string())
                    .and_then(|result| result)
                    .and_then(|vocabulary| {
                        snippets_result
                            .map_err(|error| error.to_string())
                            .and_then(|result| result)
                            .map(|snippets| (vocabulary, snippets))
                    });
                if generation() != stamp { return; }
                match result {
                    Ok((vocabulary_rows, snippet_rows)) => {
                        vocabulary.set(vocabulary_rows);
                        snippets.set(snippet_rows);
                        error.set(None);
                    }
                    Err(message) => error.set(Some(message)),
                }
                loading.set(false);
            });
        }
    };
    let event_load = load.clone();
    let event_bus = bus.clone();
    use_future(move || {
        let mut load = event_load.clone();
        let mut changes = event_bus.changed.subscribe();
        load();
        async move {
            loop {
                match changes.recv().await {
                    Ok(DICTIONARY_CHANGED) => load(),
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {},
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    });
    let focus_load = load.clone();
    ui::use_on_focus(focus_load);

    let is_snippets = tab();
    let needle = query().trim().to_lowercase();
    let source = if is_snippets { snippets() } else { vocabulary() };
    let rows: Vec<_> = source.into_iter().filter(|entry| needle.is_empty() || entry.phrase.to_lowercase().contains(&needle) || entry.replacement.as_deref().unwrap_or("").to_lowercase().contains(&needle)).collect();
    let mut start_draft = move || draft.set(Some(Draft { entry: None, snippet: tab(), phrase: String::new(), replacement: String::new() }));
    let mut start_draft_for_empty = start_draft;

    let mut commit = {
        let state = state.clone();
        let load = load.clone();
        move || {
            let Some(current) = draft() else { return; };
            draft.set(None);
            let state = state.clone();
            let mut load = load.clone();
            spawn(async move {
                let phrase = current.phrase.trim().to_string();
                let replacement = current.replacement.trim();
                let input = ops::DictionaryInput { id: current.entry.as_ref().map(|e| e.id.clone()).unwrap_or_default(), phrase, replacement: if replacement.is_empty() { None } else { Some(replacement.to_string()) }, is_snippet: current.snippet };
                let result = tokio::task::spawn_blocking(move || if current.entry.is_some() { ops::dictionary_update(&state, input) } else { ops::dictionary_add(&state, input).map(|_| ()) }).await.map_err(|e| e.to_string()).and_then(|r| r);
                if let Err(e) = result { error.set(Some(e)); } else { load(); }
            });
        }
    };
    let mut commit_for_enter = commit.clone();
    let mut commit_for_replacement = commit.clone();
    let remove = {
        let state = state.clone(); let load = load.clone();
        move |_| {
            if let Some(entry) = delete() {
                delete.set(None);
                let state = state.clone(); let mut load = load.clone();
                spawn(async move {
                    let id = entry.id;
                    let result = tokio::task::spawn_blocking(move || ops::dictionary_delete(&state, &id)).await.map_err(|e| e.to_string()).and_then(|r| r);
                    if let Err(e) = result { error.set(Some(e)); } else { load(); }
                });
            }
        }
    };
    let import = {
        let state = state.clone(); let load = load.clone();
        move |_| {
            let state = state.clone(); let mut load = load.clone();
            spawn(async move {
                let Some(path) = ui::pick_csv().await else { return; };
                let result = tokio::task::spawn_blocking(move || ops::dictionary_import_csv(&state, &path)).await.map_err(|e| e.to_string()).and_then(|r| r);
                match result {
                    Ok(result) => {
                        load();
                        import_result.set(Some(if result.errors.is_empty() { format!("Imported {} entries.", result.imported) } else { format!("Imported {} entries with {} errors:\n{}", result.imported, result.errors.len(), result.errors.iter().take(5).cloned().collect::<Vec<_>>().join("\n")) }));
                    }
                    Err(e) => error.set(Some(e)),
                }
            });
        }
    };

    rsx! {
        div { class: "flex min-h-0 flex-col gap-4",
            div { class: "flex items-center justify-between",
                div { class: "inline-flex rounded-lg border border-line bg-control p-1",
                    for (value, label) in [(false, "Vocabulary"), (true, "Snippets")] {
                        button { class: if tab() == value { "rounded-md bg-surface px-3 py-1.5 text-[13px] text-fg shadow-card" } else { "rounded-md px-3 py-1.5 text-[13px] text-fg-muted" }, onclick: move |_| tab.set(value), "{label}" }
                    }
                }
                div { class: "flex items-center gap-2",
                    if is_snippets { crate::components::button::Button { variant: crate::components::button::ButtonVariant::Secondary, onclick: import, Icon { name: IconName::Import, size: 16 } "Import CSV" } }
                    crate::components::button::Button { onclick: move |_| start_draft(), Icon { name: IconName::Plus, size: 16 } if is_snippets { "Add Snippet" } else { "Add Word" } }
                }
            }
            div { class: "max-w-sm", SearchField { value: query(), placeholder: if is_snippets { "Search snippets…" } else { "Search vocabulary…" }, oninput: move |event: FormEvent| query.set(event.value()) } }
            if !rows.is_empty() { if let Some(message) = error() { div { class: "flex items-center justify-between rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger", role: "alert", "{message}" button { class: "underline", onclick: move |_| error.set(None), "Dismiss" } } } }
            if loading() && rows.is_empty() { EmptyState { icon: IconName::Book, title: "Loading…" } }
            else if error().is_some() && rows.is_empty() { div { class: "grid justify-items-center gap-3 py-12 text-center", Icon { name: IconName::Warning, size: 36 } p { class: "m-0 text-sm text-fg-muted", "Couldn't load the dictionary" } p { class: "m-0 text-xs text-danger", "{error().unwrap_or_default()}" } button { class: "rounded-md border border-line px-3 py-1.5 text-sm", onclick: move |_| load(), "Retry" } } }
            else if rows.is_empty() && !query().trim().is_empty() { EmptyState { icon: IconName::Search, title: format!("No results for \"{}\"", query()) } }
            else if rows.is_empty() { EmptyState { icon: if is_snippets { IconName::Snippet } else { IconName::Book }, title: if is_snippets { "No snippets yet" } else { "No vocabulary words yet" }, action_label: Some(if is_snippets { "Add Snippet".into() } else { "Add Word".into() }), on_action: Some(EventHandler::new(move |_| start_draft_for_empty())) } }
            else {
                div { class: "flex flex-col gap-2",
                    for entry in rows.iter() {
                        div { class: "group relative flex min-h-16 items-center gap-3 rounded-lg border border-line bg-surface px-4 py-3 shadow-card", oncontextmenu: { let entry = entry.clone(); move |event| { event.prevent_default(); let p = event.client_coordinates(); menu.set(Some((p.x, p.y, entry.clone()))); } },
                            div { class: "min-w-0 flex-1 cursor-pointer", onclick: { let entry = entry.clone(); move |_| draft.set(Some(Draft { snippet: entry.is_snippet, phrase: entry.phrase.clone(), replacement: entry.replacement.clone().unwrap_or_default(), entry: Some(entry.clone()) })) },
                                p { class: if is_snippets { "m-0 truncate text-[13px] font-medium text-accent" } else { "m-0 truncate text-[13px] font-medium" }, "{entry.phrase}" }
                                if let Some(replacement) = &entry.replacement { p { class: if is_snippets { "mb-0 mt-0.5 line-clamp-2 text-xs text-fg-muted" } else { "mb-0 mt-0.5 truncate text-xs text-fg-muted" }, "{replacement}" } }
                            }
                            if !is_snippets {
                                if let Some(source) = &entry.source { span { class: "rounded bg-selected px-1.5 py-0.5 text-xs text-accent", "{source}" } }
                                if entry.frequency_used > 0 { span { class: "text-xs text-fg-muted", "{entry.frequency_used}x" } }
                                span { class: "text-xs text-fg-subtle", {chrono::DateTime::<chrono::Local>::from(std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(entry.modified_at.max(0.0))).format("%-m/%-d/%y").to_string()} }
                            }
                            button { class: "rounded p-1 text-fg-subtle hover:bg-control-hover", "aria-label": "More actions", onclick: { let entry = entry.clone(); move |event| { let p = event.client_coordinates(); menu.set(Some((p.x, p.y, entry.clone()))); } }, "⋯" }
                        }
                    }
                }
            }
        }
        if let Some((x, y, entry)) = menu() {
            div { class: "fixed inset-0 z-40", onclick: move |_| menu.set(None),
                div { class: "fixed z-50 min-w-36 rounded-md border border-line bg-elevated p-1 shadow-pop", style: "left: {x}px; top: {y}px", onclick: move |event| event.stop_propagation(),
                    button { class: "block w-full rounded px-3 py-1.5 text-left text-sm hover:bg-control-hover", onclick: { let entry = entry.clone(); move |_| { menu.set(None); draft.set(Some(Draft { snippet: entry.is_snippet, phrase: entry.phrase.clone(), replacement: entry.replacement.clone().unwrap_or_default(), entry: Some(entry.clone()) })); } }, "Edit" }
                    div { class: "my-1 border-t border-line" }
                    button { class: "block w-full rounded px-3 py-1.5 text-left text-sm text-danger hover:bg-control-hover", onclick: move |_| { menu.set(None); delete.set(Some(entry.clone())); }, "Delete" }
                }
            }
        }
        if let Some(current) = draft() {
            div { class: "fixed inset-0 z-40 grid place-items-center bg-black/30", onkeydown: move |event| { if event.key() == Key::Escape { event.stop_propagation(); draft.set(None); } },
                div { class: "flex max-h-[90vh] w-[380px] flex-col gap-3 rounded-xl border border-line bg-surface p-6 shadow-modal", style: if current.snippet { "width:420px" } else { "width:380px" }, onclick: move |event| event.stop_propagation(),
                    h2 { class: "m-0 text-lg font-semibold", if current.entry.is_none() { if current.snippet { "Add Snippet" } else { "Add Vocabulary Word" } } else if current.snippet { "Edit Snippet" } else { "Edit Vocabulary Word" } }
                    input { class: "rounded-md border border-line bg-window px-3 py-2 text-[13px] text-fg", maxlength: "60", placeholder: if current.snippet { if current.entry.is_none() { "Abbreviation (max 60 chars)" } else { "Abbreviation" } } else if current.entry.is_none() { "Word or phrase (max 60 chars)" } else { "Word or phrase" }, "aria-label": if current.snippet { "Abbreviation" } else { "Word or phrase" }, value: "{current.phrase}", oninput: move |event| { if let Some(mut value) = draft() { value.phrase = event.value(); draft.set(Some(value)); } }, onkeydown: move |event| { if event.key() == Key::Enter { event.prevent_default(); commit_for_enter(); } } }
                    if current.snippet {
                        label { class: "text-xs text-fg-muted", "Expansion" }
                        textarea { class: "h-28 resize-none rounded-md border border-line bg-window px-3 py-2 text-[13px] text-fg", maxlength: "4000", value: "{current.replacement}", oninput: move |event| { if let Some(mut value) = draft() { value.replacement = event.value(); draft.set(Some(value)); } } }
                    } else { input { class: "rounded-md border border-line bg-window px-3 py-2 text-[13px] text-fg", maxlength: "200", placeholder: "Replacement (optional)", "aria-label": "Replacement", value: "{current.replacement}", oninput: move |event| { if let Some(mut value) = draft() { value.replacement = event.value(); draft.set(Some(value)); } }, onkeydown: move |event| { if event.key() == Key::Enter { event.prevent_default(); commit_for_replacement(); } } } }
                    if !current.snippet { if let Some(warning) = ui::format::keyterm_warning(&current.phrase) { p { class: "m-0 flex gap-2 text-xs text-warning", Icon { name: IconName::Warning, size: 16 } "{warning}" } } }
                    div { class: "mt-2 flex justify-end gap-2",
                        crate::components::button::Button { variant: crate::components::button::ButtonVariant::Secondary, onclick: move |_| draft.set(None), "Cancel" }
                        crate::components::button::Button { disabled: current.phrase.trim().is_empty() || (current.entry.is_none() && current.snippet && current.replacement.trim().is_empty()), onclick: move |_| commit(), if current.entry.is_none() { "Add" } else { "Save" } }
                    }
                }
            }
        }
        if delete().is_some() { dialog { class: "fixed inset-0 z-50 m-auto rounded-xl border border-line bg-surface p-6 shadow-modal", open: true, h2 { "Delete this entry?" } p { "This action cannot be undone." } div { class: "flex justify-end gap-2", button { onclick: move |_| delete.set(None), "Cancel" } button { class: "text-danger", onclick: remove, "Delete" } } } }
        if let Some(result) = import_result() { dialog { class: "fixed inset-0 z-50 m-auto rounded-xl border border-line bg-surface p-6 shadow-modal", open: true, h2 { "Import Complete" } p { style: "white-space: pre-wrap", "{result}" } button { onclick: move |_| import_result.set(None), "OK" } } }
    }
}
