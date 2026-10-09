use std::sync::Arc;

use dioxus::prelude::*;
use wl_core::db::models::NoteEntry;
use wl_shell::{ops, state::AppState};

use crate::ui::{
    self,
    icons::{Icon, IconName},
    widgets::{EmptyState, SearchField},
};

#[derive(Clone, PartialEq)]
struct Editing {
    id: String,
    title: String,
    content: String,
}

#[component]
pub fn view() -> Element {
    let state = use_context::<Arc<AppState>>();
    let mut notes = use_signal(Vec::<NoteEntry>::new);
    let mut loading = use_signal(|| true);
    let mut error = use_signal(|| None::<String>);
    let mut query = use_signal(String::new);
    let mut editing = use_signal(|| None::<Editing>);
    let mut menu = use_signal(|| None::<(f64, f64, NoteEntry)>);
    let mut generation = use_signal(|| 0_u64);

    let load = {
        let state = state.clone();
        move || {
            let stamp = generation() + 1;
            generation.set(stamp);
            loading.set(true);
            let state = state.clone();
            let query = query().trim().to_string();
            spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    ops::notes_list(&state, if query.is_empty() { None } else { Some(query) })
                })
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r);
                if generation() != stamp {
                    return;
                }
                match result {
                    Ok(rows) => {
                        notes.set(rows);
                        error.set(None);
                    }
                    Err(e) => error.set(Some(e)),
                }
                loading.set(false);
            });
        }
    };
    {
        let mut load = load.clone();
        use_future(move || {
            load();
            async move { std::future::pending::<()>().await }
        });
    }
    let focus_load = load.clone();
    ui::use_on_focus(focus_load);

    let create = {
        let state = state.clone();
        let load = load.clone();
        move |_| {
            let state = state.clone();
            let mut load = load.clone();
            spawn(async move {
                let result = tokio::task::spawn_blocking(move || ops::notes_add(&state, "", ""))
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r);
                match result {
                    Ok(note) => {
                        load();
                        editing.set(Some(Editing {
                            id: note.id,
                            title: note.title,
                            content: note.content,
                        }));
                    }
                    Err(e) => error.set(Some(e)),
                }
            });
        }
    };
    let mut save = {
        let state = state.clone();
        let load = load.clone();
        move || {
            let Some(draft) = editing() else {
                return;
            };
            editing.set(None);
            let state = state.clone();
            let mut load = load.clone();
            spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    ops::notes_update(&state, &draft.id, &draft.title, &draft.content)
                })
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r);
                if let Err(e) = result {
                    error.set(Some(e));
                } else {
                    load();
                }
            });
        }
    };
    let remove = {
        let state = state.clone();
        let load = load.clone();
        move |id: String| {
            let state = state.clone();
            let mut load = load.clone();
            spawn(async move {
                let result = tokio::task::spawn_blocking(move || ops::notes_delete(&state, &id))
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r);
                if let Err(e) = result {
                    error.set(Some(e));
                } else {
                    load();
                }
            });
        }
    };
    let rows = notes();
    let create_for_empty = create.clone();
    let mut save_for_enter = save.clone();
    let mut load_for_search = load.clone();
    let mut load_for_retry = load.clone();
    rsx! {
        div { class: "flex min-h-0 flex-col gap-4",
            div { class: "flex items-center justify-between gap-3",
                div { class: "max-w-sm flex-1", SearchField { value: query(), placeholder: "Search notes…", oninput: move |event: FormEvent| { query.set(event.value()); load_for_search(); } } }
                crate::components::button::Button { onclick: create, Icon { name: IconName::Plus, size: 16 } "New Note" }
            }
            if !rows.is_empty() { if let Some(message) = error() { div { class: "flex items-center justify-between rounded-md border border-danger/30 bg-danger/10 px-3 py-2 text-sm text-danger", role: "alert", "{message}" button { class: "underline", onclick: move |_| error.set(None), "Dismiss" } } } }
            if loading() && rows.is_empty() { EmptyState { icon: IconName::Note, title: "Loading…" } }
            else if error().is_some() && rows.is_empty() { div { class: "grid justify-items-center gap-3 py-12 text-center", Icon { name: IconName::Warning, size: 36 } p { class: "m-0 text-sm text-fg-muted", "Couldn't load notes" } p { class: "m-0 text-xs text-danger", "{error().unwrap_or_default()}" } button { class: "rounded-md border border-line px-3 py-1.5 text-sm", onclick: move |_| load_for_retry(), "Retry" } } }
            else if rows.is_empty() && !query().trim().is_empty() { EmptyState { icon: IconName::Search, title: format!("No results for \"{}\"", query()) } }
            else if rows.is_empty() { EmptyState { icon: IconName::Note, title: "No notes yet", action_label: Some("Create Note".into()), on_action: Some(EventHandler::new(create_for_empty)) } }
            else {
                div { class: "flex flex-col gap-2",
                    for note in rows.iter() {
                        div { class: "group relative flex min-h-16 items-center rounded-lg border border-line bg-surface px-4 py-3 shadow-card", oncontextmenu: { let note = note.clone(); move |event| { event.prevent_default(); let p = event.client_coordinates(); menu.set(Some((p.x, p.y, note.clone()))); } },
                            div { class: "min-w-0 flex-1 cursor-pointer", onclick: { let note = note.clone(); move |_| editing.set(Some(Editing { id: note.id.clone(), title: note.title.clone(), content: note.content.clone() })) },
                                div { class: "flex items-baseline gap-2",
                                    span { class: "min-w-0 flex-1 truncate text-[13px] font-medium", if note.title.is_empty() { "Untitled" } else { "{note.title}" } }
                                    span { class: "shrink-0 text-xs text-fg-subtle", {chrono::DateTime::<chrono::Local>::from(std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(note.modified_at.max(0.0))).format("%-m/%-d/%y, %-I:%M %p").to_string()} }
                                }
                                if !note.content_preview.is_empty() { p { class: "mb-0 mt-1 line-clamp-2 text-xs text-fg-muted", "{note.content_preview}" } }
                            }
                            button { class: "ml-2 rounded p-1 text-fg-subtle hover:bg-control-hover", "aria-label": "More actions", onclick: { let note = note.clone(); move |event| { let p = event.client_coordinates(); menu.set(Some((p.x, p.y, note.clone()))); } }, "⋯" }
                        }
                    }
                }
            }
        }
        if let Some((x, y, note)) = menu() {
            div { class: "fixed inset-0 z-40", onclick: move |_| menu.set(None),
                div { class: "fixed z-50 min-w-36 rounded-md border border-line bg-elevated p-1 shadow-pop", style: "left: {x}px; top: {y}px", onclick: move |event| event.stop_propagation(),
                    button { class: "block w-full rounded px-3 py-1.5 text-left text-sm hover:bg-control-hover", onclick: { let note = note.clone(); move |_| { menu.set(None); editing.set(Some(Editing { id: note.id.clone(), title: note.title.clone(), content: note.content.clone() })); } }, "Edit" }
                    div { class: "my-1 border-t border-line" }
                    button { class: "block w-full rounded px-3 py-1.5 text-left text-sm text-danger hover:bg-control-hover", onclick: { let id = note.id.clone(); move |_| { menu.set(None); remove(id.clone()); } }, "Delete" }
                }
            }
        }
        if let Some(current) = editing() {
            div { class: "fixed inset-0 z-40 grid place-items-center bg-black/30",
                div { class: "flex h-[400px] w-[500px] flex-col gap-2 rounded-xl border border-line bg-surface p-6 shadow-modal", onclick: move |event| event.stop_propagation(), onkeydown: move |event| { if event.key() == Key::Escape { event.stop_propagation(); editing.set(None); } },
                    h2 { class: "sr-only", if current.title.is_empty() { "New Note" } else { "{current.title}" } }
                    input { class: "shrink-0 rounded-md border border-line bg-window px-2 py-1.5 text-lg text-fg", placeholder: "Title", "aria-label": "Title", value: "{current.title}", oninput: move |event| { if let Some(mut value) = editing() { value.title = event.value(); editing.set(Some(value)); } }, onkeydown: move |event| { if event.key() == Key::Enter { event.prevent_default(); save_for_enter(); } } }
                    textarea { class: "min-h-[200px] flex-1 resize-none rounded-md border border-line bg-window p-2 text-[13px] text-fg", "aria-label": "Note", value: "{current.content}", oninput: move |event| { if let Some(mut value) = editing() { value.content = event.value(); editing.set(Some(value)); } } }
                    div { class: "flex justify-end gap-2",
                        crate::components::button::Button { variant: crate::components::button::ButtonVariant::Secondary, onclick: move |_| editing.set(None), "Cancel" }
                        crate::components::button::Button { onclick: move |_| save(), "Save" }
                    }
                }
            }
        }
    }
}
